# 액터 카드 수입 도넛 추가 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 각 액터 도넛 카드의 "수입 ₩X" 텍스트 줄을 동일 크기의 수입 도넛(카테고리 범례 없음)으로 교체한다.

**Architecture:** 백엔드 `/api/summary/income` 응답에 `categories: CategorySummary[]` 를 additive 하게 추가. 프론트는 새 빌더(`buildActorIncomeSlices` / `buildHouseholdIncomeSlices`) 로 income `ActorDonutData` 를 만들어 `ActorDonut` 의 두 도넛 슬롯(income/expense)에 주입.

**Tech Stack:** Rust(axum + sqlx), Next.js 15 + React, Recharts, Vitest, cargo test.

**Spec:** `docs/superpowers/specs/2026-05-08-actor-income-donut-design.md`

---

## File Structure

**Modify:**
- `server/src/api/income.rs` — `IncomeResponse` 에 `categories` 필드 추가, SQL 한 번 더 실행
- `server/tests/test_income_split.rs` — categories breakdown 테스트 2개 추가
- `web/lib/schemas.ts` — `IncomeResponseSchema` 에 `categories` 추가
- `web/lib/donut-data.ts` — `buildActorIncomeSlices` / `buildHouseholdIncomeSlices` 추가
- `web/components/actor-donut.tsx` — props 변경(`income: number` → `income: ActorDonutData`), 텍스트 줄 → 도넛
- `web/components/dashboard-donuts.tsx` — income 빌더 사용, ActorDonut 호출 변경
- `web/__tests__/donut-data.test.ts` — 새 빌더 테스트 추가
- `web/__tests__/dashboard.test.tsx` — `donut-income` 텍스트 단언 → `donut-income-chart` / `donut-income-center` 단언으로 교체

**No changes:** DB schema, migrations, `incomeFor` (사용처 사라지지만 본 PR scope 외).

---

## Task 1: 백엔드 — IncomeResponse 에 categories 필드 추가

**Files:**
- Modify: `server/src/api/income.rs`
- Modify: `server/tests/test_income_split.rs`

### Step 1.1: 실패 테스트 작성 — categories 필드 존재 + 값

`server/tests/test_income_split.rs` 파일 끝에 다음 테스트 두 개 추가.

- [ ] **Step 1.1: 테스트 작성**

```rust
/// `categories` 필드는 income kind 카테고리만 포함하고 액터 셀 합계가 양수.
#[sqlx::test(migrations = "./migrations")]
async fn income_response_includes_categories_breakdown(pool: PgPool) {
    let owner_id = Uuid::new_v4();
    let pool = Arc::new(pool);

    let actor_eonga: Uuid = sqlx::query_scalar!(
        "INSERT INTO ledger_actors (owner_id, name) VALUES ($1, '엉아') RETURNING id",
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let salary_cat: Uuid = sqlx::query_scalar!(
        "INSERT INTO categories (owner_id, name, kind) VALUES ($1, '급여', 'income') RETURNING id",
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let batch_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO import_batches (owner_id, file_name, file_hash, year, month, row_count)
           VALUES ($1, 'cat.xlsx', '\x02'::bytea, 2026, 2, 1)
           RETURNING id"#,
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let group_id = Uuid::new_v4();
    let raw_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO transactions_raw
           (owner_id, import_batch_id, row_index, group_id, is_group_header)
           VALUES ($1, $2, 0, $3, true) RETURNING id"#,
        owner_id, batch_id, group_id,
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    sqlx::query!(
        r#"INSERT INTO transactions
           (owner_id, raw_id, group_id, occurred_on, actor_id, category_id, amount)
           VALUES ($1, $2, $3, '2026-02-25', $4, $5, 4500000)"#,
        owner_id, raw_id, group_id, actor_eonga, salary_cat,
    )
    .execute(&*pool)
    .await
    .unwrap();

    let app = build_test_router(pool, owner_id);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/summary/income/2026/2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    let categories = body["categories"].as_array().expect("categories must be an array");
    assert_eq!(categories.len(), 1, "only one income category expected");
    assert_eq!(categories[0]["category_name"], "급여");
    assert_eq!(categories[0]["kind"], "income");

    let by_actor = categories[0]["by_actor"].as_array().unwrap();
    assert_eq!(by_actor.len(), 1);
    assert_eq!(by_actor[0]["actor_name"], "엉아");
    let amt: rust_decimal::Decimal = by_actor[0]["amount"].as_str().unwrap().parse().unwrap();
    assert_eq!(amt, "4500000".parse::<rust_decimal::Decimal>().unwrap(),
               "income amount stays positive (no sign flip)");
}

/// expense kind 카테고리는 categories 에 절대 등장하지 않는다.
#[sqlx::test(migrations = "./migrations")]
async fn income_categories_exclude_expense_kind(pool: PgPool) {
    let owner_id = Uuid::new_v4();
    let pool = Arc::new(pool);

    let actor: Uuid = sqlx::query_scalar!(
        "INSERT INTO ledger_actors (owner_id, name) VALUES ($1, '엉아') RETURNING id",
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let income_cat: Uuid = sqlx::query_scalar!(
        "INSERT INTO categories (owner_id, name, kind) VALUES ($1, '급여', 'income') RETURNING id",
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let expense_cat: Uuid = sqlx::query_scalar!(
        "INSERT INTO categories (owner_id, name, kind) VALUES ($1, '식비', 'expense') RETURNING id",
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    let batch_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO import_batches (owner_id, file_name, file_hash, year, month, row_count)
           VALUES ($1, 'mix.xlsx', '\x03'::bytea, 2026, 2, 2)
           RETURNING id"#,
        owner_id
    )
    .fetch_one(&*pool)
    .await
    .unwrap();

    for (i, (cat, amount)) in [(income_cat, 1000000_i64), (expense_cat, -50000_i64)].iter().enumerate() {
        let group_id = Uuid::new_v4();
        let raw_id: Uuid = sqlx::query_scalar!(
            r#"INSERT INTO transactions_raw
               (owner_id, import_batch_id, row_index, group_id, is_group_header)
               VALUES ($1, $2, $3, $4, true) RETURNING id"#,
            owner_id, batch_id, i as i32, group_id,
        )
        .fetch_one(&*pool)
        .await
        .unwrap();

        sqlx::query!(
            r#"INSERT INTO transactions
               (owner_id, raw_id, group_id, occurred_on, actor_id, category_id, amount)
               VALUES ($1, $2, $3, '2026-02-15', $4, $5, $6)"#,
            owner_id, raw_id, group_id, actor, cat, rust_decimal::Decimal::from(*amount),
        )
        .execute(&*pool)
        .await
        .unwrap();
    }

    let app = build_test_router(pool, owner_id);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/summary/income/2026/2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    let categories = body["categories"].as_array().unwrap();
    assert_eq!(categories.len(), 1, "only the income category should appear");
    assert_eq!(categories[0]["category_name"], "급여");
    assert_eq!(categories[0]["kind"], "income");
    for c in categories {
        assert_ne!(c["kind"], "expense", "expense kind must not leak into income response");
    }
}
```

### Step 1.2: 테스트 실행으로 실패 확인

- [ ] **Step 1.2:** Run: `cd server && cargo test -p server income_response_includes_categories_breakdown income_categories_exclude_expense_kind --no-fail-fast`
  Expected: 컴파일은 됨(타입 자체는 `serde_json::Value` 로 접근하므로). 둘 다 `categories must be an array` 또는 length 0 으로 FAIL.
  실제로 `categories` 키 부재 시 `body["categories"].as_array()` → `None` → expect panic.

### Step 1.3: `IncomeResponse` 확장 + SQL 추가

`server/src/api/income.rs` 를 다음과 같이 수정.

- [ ] **Step 1.3: 코드 작성**

```rust
use axum::{
    extract::{Path, State},
    Json,
};
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::api::summary::{ByActorEntry, CategorySummary};
use crate::auth::ExtractUser;
use crate::error::AppResult;

#[derive(Debug, Serialize)]
pub struct IncomeByActor {
    pub actor_id: Option<Uuid>,
    pub actor_name: String,
    pub total: Decimal,
}

#[derive(Debug, Serialize)]
pub struct IncomeResponse {
    /// "YYYY-MM" 형식.
    pub month: String,
    pub by_actor: Vec<IncomeByActor>,
    pub total: Decimal,
    /// 수입 카테고리 × 액터 분해. expense summary 와 동일 구조이나 amount 는 부호 그대로 양수.
    pub categories: Vec<CategorySummary>,
}

/// GET /api/summary/income/:year/:month
///
/// 해당 월의 `kind='income'` 카테고리 트랜잭션을 액터별로 합산한다.
/// 저장 규약상 수입은 양수이므로 그대로 SUM(amount).
/// 등록된 모든 액터를 by_actor 에 포함하되 거래 없는 액터는 total=0 으로 채운다.
/// `categories` 는 income kind 카테고리만 포함하며 expense summary 와 동일 셰이프.
pub async fn handle_get_income(
    State(pool): State<Arc<PgPool>>,
    ExtractUser(user): ExtractUser,
    Path((year, month)): Path<(i32, i32)>,
) -> AppResult<Json<IncomeResponse>> {
    let owner_id = user.sub;

    let by_actor_rows = sqlx::query!(
        r#"
        SELECT
            a.id   AS "actor_id!: Uuid",
            a.name AS "actor_name!: String",
            COALESCE(
                (SELECT SUM(t.amount)
                 FROM transactions t
                 JOIN categories c ON c.id = t.category_id AND c.owner_id = t.owner_id
                 WHERE t.owner_id = $1
                   AND t.actor_id = a.id
                   AND c.kind = 'income'
                   AND t.occurred_on >= make_date($2, $3, 1)
                   AND t.occurred_on  < make_date($2, $3, 1) + INTERVAL '1 month'),
                0::numeric(15,2)
            ) AS "total!: Decimal"
        FROM ledger_actors a
        WHERE a.owner_id = $1
        ORDER BY a.name
        "#,
        owner_id,
        year,
        month,
    )
    .fetch_all(&*pool)
    .await?;

    let by_actor: Vec<IncomeByActor> = by_actor_rows
        .into_iter()
        .map(|r| IncomeByActor {
            actor_id: Some(r.actor_id),
            actor_name: r.actor_name,
            total: r.total,
        })
        .collect();

    let total: Decimal = by_actor.iter().map(|e| e.total).sum();

    // ── categories breakdown (income kind, sign preserved) ──────────────────
    let cat_rows = sqlx::query!(
        r#"
        SELECT
            c.id        AS "category_id!: Uuid",
            c.name      AS "category_name!: String",
            c.kind      AS "kind!: String",
            a.id        AS "actor_id?: Uuid",
            a.name      AS "actor_name?: String",
            (SUM(t.amount))::numeric(15,2) AS "amount!: Decimal"
        FROM transactions t
        JOIN categories c         ON c.id = t.category_id AND c.owner_id = t.owner_id
        LEFT JOIN ledger_actors a ON a.id = t.actor_id    AND a.owner_id = t.owner_id
        WHERE t.owner_id = $1
          AND c.kind = 'income'
          AND t.occurred_on >= make_date($2, $3, 1)
          AND t.occurred_on  < make_date($2, $3, 1) + INTERVAL '1 month'
        GROUP BY c.id, c.name, c.kind, a.id, a.name
        ORDER BY c.name, a.name
        "#,
        owner_id,
        year,
        month,
    )
    .fetch_all(&*pool)
    .await?;

    let mut category_order: Vec<Uuid> = Vec::new();
    let mut category_meta: HashMap<Uuid, (String, String)> = HashMap::new();
    let mut category_actors: HashMap<Uuid, Vec<ByActorEntry>> = HashMap::new();

    for row in cat_rows {
        let actor_id = row.actor_id;
        let actor_name = row.actor_name.unwrap_or_else(|| "(미지정)".to_string());

        if !category_meta.contains_key(&row.category_id) {
            category_order.push(row.category_id);
            category_meta.insert(row.category_id, (row.category_name.clone(), row.kind.clone()));
        }

        category_actors
            .entry(row.category_id)
            .or_default()
            .push(ByActorEntry {
                actor_id,
                actor_name,
                amount: row.amount,
            });
    }

    let categories: Vec<CategorySummary> = category_order
        .into_iter()
        .map(|cid| {
            let (name, kind) = category_meta.remove(&cid).unwrap();
            let by_actor = category_actors.remove(&cid).unwrap_or_default();
            let cat_total: Decimal = by_actor
                .iter()
                .map(|e| e.amount)
                .fold(Decimal::ZERO, |acc, x| acc + x);
            CategorySummary {
                category_id: cid,
                category_name: name,
                kind,
                by_actor,
                total: cat_total,
            }
        })
        .collect();

    Ok(Json(IncomeResponse {
        month: format!("{:04}-{:02}", year, month),
        by_actor,
        total,
        categories,
    }))
}
```

`ByActorEntry` 와 `CategorySummary` 의 가시성을 확인. `server/src/api/summary.rs` 에서 이미 `pub struct` 로 선언되어 있으므로 `pub use` 없이 직접 `crate::api::summary::{ByActorEntry, CategorySummary}` import 가능.

### Step 1.4: sqlx offline 캐시 재생성

- [ ] **Step 1.4: sqlx prepare 실행**

Run: `cd server && cargo sqlx prepare -- --tests` (또는 프로젝트의 `sqlx.sh` 가 있다면 `./sqlx.sh`)
Expected: `.sqlx/` 디렉토리에 새 query 해시 파일 생성/갱신.

### Step 1.5: 테스트 통과 확인

- [ ] **Step 1.5:** Run: `cd server && cargo test -p server`
  Expected: 신규 2개 포함 전체 PASS (이전 85 → 87).

### Step 1.6: 커밋

- [ ] **Step 1.6: 커밋**

```bash
git add server/src/api/income.rs server/tests/test_income_split.rs server/.sqlx
git commit -m "feat(server): add income categories breakdown to /api/summary/income"
```

---

## Task 2: 프론트엔드 — schemas + donut-data 빌더

**Files:**
- Modify: `web/lib/schemas.ts`
- Modify: `web/lib/donut-data.ts`
- Modify: `web/__tests__/donut-data.test.ts`

### Step 2.1: 실패 테스트 작성 — `buildActorIncomeSlices` / `buildHouseholdIncomeSlices`

`web/__tests__/donut-data.test.ts` 의 import 라인을 다음으로 교체:

```ts
import {
  buildActorSlices,
  buildHouseholdSlices,
  buildDeductionByActor,
  buildActorIncomeSlices,
  buildHouseholdIncomeSlices,
  incomeFor,
  collectOrderedActorIds,
  EXPENSE_PALETTE,
  OTHER_COLOR,
  DEDUCTION_PALETTE,
} from "../lib/donut-data";
```

파일 끝(마지막 `describe` 다음)에 추가:

- [ ] **Step 2.1: 테스트 작성**

```ts
describe("buildActorIncomeSlices (수입 도넛)", () => {
  function makeIncome(
    cats: Array<{ name: string; cells: Array<{ actor: string | null; actorName: string; amount: string }> }>,
  ): IncomeResponse {
    return {
      month: "2026-02",
      by_actor: [],
      total: "0",
      categories: cats.map((c, i) => ({
        category_id: `${"2".repeat(8)}-2222-2222-2222-${String(i).padStart(12, "0")}`,
        category_name: c.name,
        kind: "income",
        by_actor: c.cells.map((cell) => ({
          actor_id: cell.actor,
          actor_name: cell.actorName,
          amount: cell.amount,
        })),
        total: "0",
      })),
    };
  }

  it("특정 액터의 income 카테고리를 슬라이스로 반환", () => {
    const income = makeIncome([
      { name: "급여", cells: [{ actor: ACTOR_A, actorName: "공동", amount: "3000000" }] },
      { name: "보험금", cells: [{ actor: ACTOR_A, actorName: "공동", amount: "100000" }] },
    ]);
    const result = buildActorIncomeSlices(income, ACTOR_A);
    expect(result.actorId).toBe(ACTOR_A);
    expect(result.slices.map((s) => s.name).sort()).toEqual(["급여", "보험금"]);
    expect(result.total).toBe(3100000);
  });

  it("EXPENSE_PALETTE 를 재사용한다", () => {
    const income = makeIncome([
      { name: "급여", cells: [{ actor: ACTOR_A, actorName: "공동", amount: "1000" }] },
    ]);
    const result = buildActorIncomeSlices(income, ACTOR_A);
    expect(result.slices[0].color).toBe(EXPENSE_PALETTE[0]);
  });

  it("입력이 null 이면 빈 결과", () => {
    const result = buildActorIncomeSlices(null, ACTOR_A);
    expect(result.slices).toEqual([]);
    expect(result.total).toBe(0);
  });

  it("해당 액터 셀이 없으면 빈 슬라이스", () => {
    const income = makeIncome([
      { name: "급여", cells: [{ actor: ACTOR_B, actorName: "엉아", amount: "1000" }] },
    ]);
    const result = buildActorIncomeSlices(income, ACTOR_A);
    expect(result.slices).toEqual([]);
    expect(result.total).toBe(0);
  });

  it("7개 income 카테고리 → top-6 + 기타", () => {
    const income = makeIncome(
      Array.from({ length: 7 }, (_, i) => ({
        name: `i${i + 1}`,
        cells: [{ actor: ACTOR_A, actorName: "공동", amount: String((i + 1) * 1000) }],
      })),
    );
    const result = buildActorIncomeSlices(income, ACTOR_A);
    expect(result.slices.map((s) => s.name)).toEqual([
      "i7", "i6", "i5", "i4", "i3", "i2", "기타",
    ]);
    expect(result.slices[6].color).toBe(OTHER_COLOR);
  });
});

describe("buildHouseholdIncomeSlices (가구 합계 수입 도넛)", () => {
  function makeIncome(
    cats: Array<{ name: string; cells: Array<{ actor: string; actorName: string; amount: string }> }>,
  ): IncomeResponse {
    return {
      month: "2026-02",
      by_actor: [],
      total: "0",
      categories: cats.map((c, i) => ({
        category_id: `${"3".repeat(8)}-3333-3333-3333-${String(i).padStart(12, "0")}`,
        category_name: c.name,
        kind: "income",
        by_actor: c.cells.map((cell) => ({
          actor_id: cell.actor,
          actor_name: cell.actorName,
          amount: cell.amount,
        })),
        total: "0",
      })),
    };
  }

  it("모든 액터를 카테고리별로 합산", () => {
    const income = makeIncome([
      {
        name: "급여",
        cells: [
          { actor: ACTOR_A, actorName: "공동", amount: "1000" },
          { actor: ACTOR_B, actorName: "엉아", amount: "2000" },
        ],
      },
    ]);
    const result = buildHouseholdIncomeSlices(income);
    expect(result.actorName).toBe("가구 합계");
    expect(result.slices.map((s) => s.name)).toEqual(["급여"]);
    expect(result.total).toBe(3000);
  });

  it("입력 null 이면 빈 결과", () => {
    const result = buildHouseholdIncomeSlices(null);
    expect(result.slices).toEqual([]);
    expect(result.total).toBe(0);
  });
});
```

### Step 2.2: 테스트 실패 확인

- [ ] **Step 2.2:** Run: `cd web && npm test -- donut-data`
  Expected: 새 describe 블록의 모든 테스트 FAIL (`buildActorIncomeSlices is not a function`).

### Step 2.3: `IncomeResponseSchema` 확장

`web/lib/schemas.ts` — `IncomeResponseSchema` 정의(line 278) 를 다음으로 교체:

- [ ] **Step 2.3: schema 수정**

```ts
export const IncomeResponseSchema = z.object({
  month: z.string(),
  by_actor: z.array(IncomeByActorSchema),
  total: DecimalSchema,
  categories: z.array(CategorySummarySchema),
});
```

`CategorySummarySchema` 는 같은 파일 line 129 에 이미 있음.

### Step 2.4: donut-data 에 빌더 추가

`web/lib/donut-data.ts` 끝부분(`collectOrderedActorIds` 다음)에 추가:

- [ ] **Step 2.4: 빌더 구현**

```ts
/**
 * 단일 액터의 수입 슬라이스. expense 와 다른 점:
 *   - 차감 제외 로직 불필요 (income kind 만 들어옴)
 *   - 부호 그대로 (양수)
 */
export function buildActorIncomeSlices(
  income: IncomeResponse | null,
  actorId: string | null,
): ActorDonutData {
  if (!income) {
    return { actorId, actorName: actorId ?? "미지정", total: 0, slices: [] };
  }
  const raws: ExpenseRaw[] = [];
  for (const cat of income.categories) {
    const cell = cat.by_actor.find((e) => e.actor_id === actorId);
    if (!cell) continue;
    const v = parseFloat(cell.amount);
    if (Number.isNaN(v) || v === 0) continue;
    raws.push({ name: cat.category_name, value: v });
  }
  const total = raws.reduce((acc, r) => acc + r.value, 0);
  const slices = topNWithOther(raws);
  return { actorId, actorName: actorId ?? "미지정", total, slices };
}

/**
 * 가구 전체 수입 — 모든 액터의 income 을 카테고리별로 합산.
 */
export function buildHouseholdIncomeSlices(
  income: IncomeResponse | null,
): ActorDonutData {
  if (!income) {
    return { actorId: "household", actorName: HOUSEHOLD_NAME, total: 0, slices: [] };
  }
  const sums = new Map<string, number>();
  for (const cat of income.categories) {
    let agg = 0;
    for (const cell of cat.by_actor) {
      const v = parseFloat(cell.amount);
      if (!Number.isNaN(v)) agg += v;
    }
    if (agg !== 0) sums.set(cat.category_name, agg);
  }
  const raws: ExpenseRaw[] = Array.from(sums.entries()).map(([name, value]) => ({
    name,
    value,
  }));
  const total = raws.reduce((acc, r) => acc + r.value, 0);
  const slices = topNWithOther(raws);
  return {
    actorId: "household",
    actorName: HOUSEHOLD_NAME,
    total,
    slices,
  };
}
```

### Step 2.5: 테스트 통과 확인

- [ ] **Step 2.5:** Run: `cd web && npm test -- donut-data`
  Expected: 신규 7개 포함 PASS.

### Step 2.6: 커밋

- [ ] **Step 2.6: 커밋**

```bash
git add web/lib/schemas.ts web/lib/donut-data.ts web/__tests__/donut-data.test.ts
git commit -m "feat(web): add income donut data builders + IncomeResponse.categories schema"
```

---

## Task 3: 프론트엔드 — ActorDonut 에 수입 도넛 슬롯 추가

**Files:**
- Modify: `web/components/actor-donut.tsx`
- Modify: `web/__tests__/dashboard.test.tsx`

### Step 3.1: 실패 테스트 작성 — 수입 도넛 차트 + 가운데 라벨

`web/__tests__/dashboard.test.tsx` 의 `describe("ActorDonut", ...)` 블록(line 123) 전체를 다음으로 **교체**.

먼저 import 라인(line 14) 추가:
```ts
import { buildActorSlices, buildActorIncomeSlices } from "../lib/donut-data";
```

다음 헬퍼를 ActorDonut describe 위에 추가:
```ts
const EMPTY_DONUT = { actorId: null, actorName: "공동", total: 0, slices: [] };
function makeIncomeOne(actorId: string, amount: string) {
  return {
    month: "2026-02",
    by_actor: [],
    total: "0",
    categories: [
      {
        category_id: "99999999-9999-9999-9999-999999999999",
        category_name: "급여",
        kind: "income",
        by_actor: [{ actor_id: actorId, actor_name: "공동", amount }],
        total: amount,
      },
    ],
  } satisfies IncomeResponse;
}
```

ActorDonut describe 블록을 교체:

- [ ] **Step 3.1: 테스트 교체**

```tsx
describe("ActorDonut", () => {
  const ACTOR_A = "00000000-0000-0000-0000-0000000000aa";

  it("수입/지출 모두 비면 빈 placeholder", () => {
    render(<ActorDonut expense={EMPTY_DONUT} income={EMPTY_DONUT} />);
    expect(screen.getByTestId("donut-empty")).toBeTruthy();
  });

  it("수입이 있으면 수입 도넛(차트 + 가운데 '수입 ₩X')을 렌더한다", () => {
    const incomeData = buildActorIncomeSlices(makeIncomeOne(ACTOR_A, "5741025"), ACTOR_A);
    render(<ActorDonut expense={EMPTY_DONUT} income={incomeData} />);
    expect(screen.getByTestId("donut-income-chart")).toBeTruthy();
    const center = screen.getByTestId("donut-income-center");
    expect(center.textContent).toContain("수입");
    expect(center.textContent).toContain("5,741,025");
  });

  it("수입 = 0 일 때 수입 도넛 영역 미렌더", () => {
    const data: SummaryResponse = {
      year: 2026,
      month: 2,
      actors: [{ actor_id: ACTOR_A, actor_name: "공동" }],
      categories: [
        {
          category_id: "11111111-1111-1111-1111-111111111111",
          category_name: "외식",
          kind: "expense",
          by_actor: [{ actor_id: ACTOR_A, actor_name: "공동", amount: "1000" }],
          total: "1000",
        },
      ],
    };
    render(<ActorDonut expense={buildActorSlices(data, ACTOR_A)} income={EMPTY_DONUT} />);
    expect(screen.queryByTestId("donut-income-chart")).toBeNull();
  });

  it("지출 중앙 라벨이 '지출' + 합계", () => {
    const data: SummaryResponse = {
      year: 2026,
      month: 2,
      actors: [{ actor_id: ACTOR_A, actor_name: "공동" }],
      categories: [
        {
          category_id: "11111111-1111-1111-1111-111111111111",
          category_name: "외식",
          kind: "expense",
          by_actor: [{ actor_id: ACTOR_A, actor_name: "공동", amount: "100000" }],
          total: "100000",
        },
      ],
    };
    render(<ActorDonut expense={buildActorSlices(data, ACTOR_A)} income={EMPTY_DONUT} />);
    const center = screen.getByTestId("donut-center");
    expect(center.textContent).toContain("지출");
    expect(center.textContent).toContain("100,000");
  });

  it("지출 범례 % 분모는 Σ|value|", () => {
    const data: SummaryResponse = {
      year: 2026,
      month: 2,
      actors: [{ actor_id: ACTOR_A, actor_name: "공동" }],
      categories: [
        {
          category_id: "11111111-1111-1111-1111-111111111111",
          category_name: "외식",
          kind: "expense",
          by_actor: [{ actor_id: ACTOR_A, actor_name: "공동", amount: "1000" }],
          total: "1000",
        },
        {
          category_id: "22222222-2222-2222-2222-222222222222",
          category_name: "병원",
          kind: "expense",
          by_actor: [{ actor_id: ACTOR_A, actor_name: "공동", amount: "3000" }],
          total: "3000",
        },
      ],
    };
    render(<ActorDonut expense={buildActorSlices(data, ACTOR_A)} income={EMPTY_DONUT} />);
    expect(screen.getByText(/병원/).parentElement?.parentElement?.textContent).toContain("75.0%");
    expect(screen.getByText(/외식/).parentElement?.parentElement?.textContent).toContain("25.0%");
  });

  it("수입은 있고 지출 슬라이스 0 → 수입 도넛 + '지출 없음' 텍스트", () => {
    const incomeData = buildActorIncomeSlices(makeIncomeOne(ACTOR_A, "1000"), ACTOR_A);
    render(<ActorDonut expense={EMPTY_DONUT} income={incomeData} />);
    expect(screen.getByTestId("donut-income-chart")).toBeTruthy();
    expect(screen.getByTestId("donut-no-expense")).toBeTruthy();
    expect(screen.queryByTestId("donut-empty")).toBeNull();
  });

  it("수입 도넛에 카테고리 범례(%)가 노출되지 않는다", () => {
    const incomeData = buildActorIncomeSlices(makeIncomeOne(ACTOR_A, "5000000"), ACTOR_A);
    render(<ActorDonut expense={EMPTY_DONUT} income={incomeData} />);
    // 수입 도넛 영역 안에 "급여" 카테고리명이 텍스트로 노출되면 안 됨
    expect(screen.queryByText(/급여/)).toBeNull();
  });
});
```

### Step 3.2: 테스트 실행 → 컴파일 / 단언 실패

- [ ] **Step 3.2:** Run: `cd web && npm test -- dashboard`
  Expected: ActorDonut 관련 테스트 FAIL — props (`expense`/`income`) 가 `data`/`income(number)` 와 불일치, `donut-income-chart` testid 부재.

### Step 3.3: ActorDonut 컴포넌트 재작성

`web/components/actor-donut.tsx` 전체를 다음으로 교체:

- [ ] **Step 3.3: 컴포넌트 재작성**

```tsx
"use client";

import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { PieChart, Pie, Cell, ResponsiveContainer, Tooltip } from "recharts";
import type { ActorDonutData, DonutSlice } from "@/lib/donut-data";

type Props = {
  expense: ActorDonutData;
  income: ActorDonutData;
};

function fmtSigned(v: number): string {
  const abs = Math.abs(v).toLocaleString("ko-KR");
  return v < 0 ? `-₩${abs}` : `₩${abs}`;
}

function DonutChart({
  slices,
  testIdPrefix,
  centerLabel,
  centerValue,
}: {
  slices: DonutSlice[];
  testIdPrefix: "income" | "expense";
  centerLabel: string;
  centerValue: number;
}) {
  return (
    <div className="relative h-44" data-testid={`donut-${testIdPrefix}-chart`}>
      <ResponsiveContainer width="100%" height="100%">
        <PieChart>
          <Pie
            data={slices.map((s) => ({ ...s, value: Math.abs(s.value) }))}
            dataKey="value"
            nameKey="name"
            innerRadius={48}
            outerRadius={72}
            paddingAngle={1}
            stroke="none"
          >
            {slices.map((s) => (
              <Cell key={s.name} fill={s.color} />
            ))}
          </Pie>
          <Tooltip
            formatter={(v: number) => `₩${v.toLocaleString("ko-KR")}`}
            contentStyle={{ fontSize: 12 }}
          />
        </PieChart>
      </ResponsiveContainer>
      <div
        data-testid={`donut-${testIdPrefix === "expense" ? "center" : "income-center"}`}
        className="absolute inset-0 pointer-events-none flex flex-col items-center justify-center"
      >
        <span className="text-xs text-muted-foreground">{centerLabel}</span>
        <span className="text-base font-semibold tabular-nums">
          {fmtSigned(centerValue)}
        </span>
      </div>
    </div>
  );
}

export function ActorDonut({ expense, income }: Props) {
  const expenseDenom = expense.slices.reduce((acc, s) => acc + Math.abs(s.value), 0);
  const hasIncome = income.slices.length > 0;
  const hasExpense = expense.slices.length > 0;
  const hasNothing = !hasIncome && !hasExpense;

  return (
    <Card data-testid={`actor-donut-${expense.actorName}`}>
      <CardHeader>
        <CardTitle className="text-base">{expense.actorName}</CardTitle>
      </CardHeader>
      <CardContent>
        {hasNothing ? (
          <p className="text-sm text-muted-foreground" data-testid="donut-empty">
            이 달의 거래 내역이 없습니다.
          </p>
        ) : (
          <div className="flex flex-col gap-3">
            {hasIncome && (
              <DonutChart
                slices={income.slices}
                testIdPrefix="income"
                centerLabel="수입"
                centerValue={income.total}
              />
            )}

            {hasExpense ? (
              <>
                <DonutChart
                  slices={expense.slices}
                  testIdPrefix="expense"
                  centerLabel="지출"
                  centerValue={expense.total}
                />
                <ul className="text-sm space-y-1">
                  {expense.slices.map((s: DonutSlice, i) => (
                    <li
                      key={`${s.name}-${i}`}
                      className="flex items-center justify-between gap-2"
                    >
                      <span className="flex items-center gap-2 truncate">
                        <span
                          aria-hidden="true"
                          className="inline-block h-2.5 w-2.5 rounded-sm shrink-0"
                          style={{ backgroundColor: s.color }}
                        />
                        <span className="truncate">{s.name}</span>
                      </span>
                      <span className="tabular-nums text-muted-foreground shrink-0">
                        {fmtSigned(s.value)} · {expenseDenom === 0 ? "0%" : `${((Math.abs(s.value) / expenseDenom) * 100).toFixed(1)}%`}
                      </span>
                    </li>
                  ))}
                </ul>
              </>
            ) : (
              <p
                data-testid="donut-no-expense"
                className="text-sm text-muted-foreground"
              >
                이 달 지출 없음.
              </p>
            )}
          </div>
        )}
      </CardContent>
    </Card>
  );
}
```

### Step 3.4: 테스트 통과 확인 (ActorDonut 만)

- [ ] **Step 3.4:** Run: `cd web && npm test -- dashboard`
  Expected: ActorDonut describe PASS. DashboardDonuts describe 는 아직 FAIL 가능 (다음 Task 에서 props 정리).

---

## Task 4: 프론트엔드 — DashboardDonuts 에서 income builder 사용

**Files:**
- Modify: `web/components/dashboard-donuts.tsx`
- Modify: `web/__tests__/dashboard.test.tsx` — DashboardDonuts describe 만

### Step 4.1: DashboardDonuts 테스트 갱신

`web/__tests__/dashboard.test.tsx` 의 `describe("DashboardDonuts ...)` 블록 (line 232–) 안 4개 테스트를 다음으로 교체:

- [ ] **Step 4.1: 테스트 갱신**

```tsx
function makeIncome(): IncomeResponse {
  return {
    month: "2026-02",
    by_actor: [
      { actor_id: A, actor_name: "공동", total: "0" },
      { actor_id: BABY, actor_name: "아기", total: "5741025" },
      { actor_id: SPOUSE, actor_name: "엉아", total: "6475664" },
    ],
    total: "12216689",
    categories: [
      {
        category_id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        category_name: "급여",
        kind: "income",
        by_actor: [
          { actor_id: BABY, actor_name: "아기", amount: "5741025" },
          { actor_id: SPOUSE, actor_name: "엉아", amount: "6475664" },
        ],
        total: "12216689",
      },
    ],
  };
}
```

(기존 `makeIncome` 정의를 위와 같이 교체. `makeSummary` 는 그대로.)

테스트 케이스 4개를 다음으로 교체:

```tsx
it("data 가 null 이면 empty 카드", () => {
  render(<DashboardDonuts summary={null} income={null} />);
  expect(screen.getByTestId("dashboard-donuts-empty")).toBeTruthy();
});

it("3장의 카드를 고정 순서 가구합계 / 아기 / 엉아 로 렌더한다", () => {
  render(<DashboardDonuts summary={makeSummary()} income={makeIncome()} />);
  const cards = screen.getAllByTestId(/^actor-donut-/);
  expect(cards[0].getAttribute("data-testid")).toBe("actor-donut-가구 합계");
  expect(cards[1].getAttribute("data-testid")).toBe("actor-donut-아기");
  expect(cards[2].getAttribute("data-testid")).toBe("actor-donut-엉아");
});

it("가구 합계 카드의 수입 도넛 중앙 라벨에 합계가 표시된다", () => {
  render(<DashboardDonuts summary={makeSummary()} income={makeIncome()} />);
  const householdCard = screen.getByTestId("actor-donut-가구 합계");
  const center = householdCard.querySelector('[data-testid="donut-income-center"]');
  expect(center?.textContent).toContain("12,216,689");
});

it("아기 카드의 수입 도넛 중앙 라벨에 by_actor 매치 값", () => {
  render(<DashboardDonuts summary={makeSummary()} income={makeIncome()} />);
  const babyCard = screen.getByTestId("actor-donut-아기");
  const center = babyCard.querySelector('[data-testid="donut-income-center"]');
  expect(center?.textContent).toContain("5,741,025");
});

it("income 0 인 공동 카드는 수입 도넛 미렌더", () => {
  // makeIncome() 에서 공동 actor 의 income 카테고리 셀이 없음
  render(<DashboardDonuts summary={makeSummary()} income={makeIncome()} />);
  // 가구 합계 카드는 수입 있음. 공동 actor 카드는 PERSON_NAMES 에 없으므로 렌더 안 됨.
  // 대신 income 0 인 케이스: 아기 actor 의 income 만 0 인 변형
  const noBabyIncome: IncomeResponse = {
    month: "2026-02",
    by_actor: [
      { actor_id: BABY, actor_name: "아기", total: "0" },
      { actor_id: SPOUSE, actor_name: "엉아", total: "1000" },
    ],
    total: "1000",
    categories: [
      {
        category_id: "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
        category_name: "급여",
        kind: "income",
        by_actor: [{ actor_id: SPOUSE, actor_name: "엉아", amount: "1000" }],
        total: "1000",
      },
    ],
  };
  const { container } = render(<DashboardDonuts summary={makeSummary()} income={noBabyIncome} />);
  const babyCard = container.querySelector('[data-testid="actor-donut-아기"]')!;
  expect(babyCard.querySelector('[data-testid="donut-income-chart"]')).toBeNull();
});
```

### Step 4.2: 테스트 실행 → DashboardDonuts FAIL

- [ ] **Step 4.2:** Run: `cd web && npm test -- dashboard`
  Expected: DashboardDonuts 관련 테스트 FAIL (현 컴포넌트가 `income(number)` 만 넘김).

### Step 4.3: DashboardDonuts 갱신

`web/components/dashboard-donuts.tsx` 전체를 다음으로 교체:

- [ ] **Step 4.3: 컴포넌트 갱신**

```tsx
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { PieChart } from "lucide-react";
import { ActorDonut } from "./actor-donut";
import {
  buildActorSlices,
  buildHouseholdSlices,
  buildActorIncomeSlices,
  buildHouseholdIncomeSlices,
} from "@/lib/donut-data";
import type { SummaryResponse, IncomeResponse } from "@/lib/schemas";

type Props = {
  summary: SummaryResponse | null;
  income: IncomeResponse | null;
};

function EmptyDonutsCard() {
  return (
    <Card data-testid="dashboard-donuts-empty">
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">
          <PieChart className="h-4 w-4" />
          카테고리 분포
        </CardTitle>
      </CardHeader>
      <CardContent>
        <p className="text-sm text-muted-foreground">
          이 달의 거래 내역이 없습니다.
        </p>
      </CardContent>
    </Card>
  );
}

const PERSON_NAMES = ["아기", "엉아"] as const;

export function DashboardDonuts({ summary, income }: Props) {
  if (!summary) return <EmptyDonutsCard />;

  const householdExpense = buildHouseholdSlices(summary);
  const householdIncome = buildHouseholdIncomeSlices(income);

  const personCards = PERSON_NAMES.map((name) => {
    const actor = summary.actors.find((a) => a.actor_name === name);
    const expense = actor
      ? buildActorSlices(summary, actor.actor_id)
      : { actorId: null, actorName: name, total: 0, slices: [] };
    const incomeData = actor
      ? buildActorIncomeSlices(income, actor.actor_id)
      : { actorId: null, actorName: name, total: 0, slices: [] };
    // expense/income 의 actorName 을 카드 제목용으로 통일
    return {
      expense: { ...expense, actorName: name },
      income: { ...incomeData, actorName: name },
    };
  });

  return (
    <div className="grid grid-cols-1 md:grid-cols-3 gap-4" data-testid="dashboard-donuts">
      <ActorDonut expense={householdExpense} income={householdIncome} />
      {personCards.map((pc) => (
        <ActorDonut key={pc.expense.actorName} expense={pc.expense} income={pc.income} />
      ))}
    </div>
  );
}
```

### Step 4.4: 전체 frontend 테스트 통과 확인

- [ ] **Step 4.4:** Run: `cd web && npm test`
  Expected: 전체 PASS. 신규 추가된 ActorDonut/Income/DashboardDonuts 케이스 포함. (이전 116 → 약 122–124, 정확 수치는 실측.)

### Step 4.5: TypeScript 빌드 확인

- [ ] **Step 4.5:** Run: `cd web && npm run build` (또는 `npx tsc --noEmit`)
  Expected: 타입 에러 없음. 만약 `incomeFor` 미사용 경고가 나면 그대로 유지(스코프 밖). 만약 `import { incomeFor }` dead import 가 다른 파일에 남아 있으면 그것만 제거.

### Step 4.6: 커밋

- [ ] **Step 4.6: 커밋**

```bash
git add web/components/actor-donut.tsx web/components/dashboard-donuts.tsx web/__tests__/dashboard.test.tsx
git commit -m "feat(web): replace income text line with same-sized income donut per actor card"
```

---

## Task 5: 통합 검증 + 문서 업데이트

**Files:**
- Modify: `CLAUDE.md` (Cumulative Context 항목 추가)

### Step 5.1: 백엔드 테스트 전체

- [ ] **Step 5.1:** Run: `cd server && cargo test -p server`
  Expected: 전체 PASS. 87+ tests.

### Step 5.2: 프론트엔드 테스트 전체

- [ ] **Step 5.2:** Run: `cd web && npm test`
  Expected: 전체 PASS.

### Step 5.3: dev 서버 수동 확인 (가능하면)

- [ ] **Step 5.3:** docker compose up 또는 로컬 dev 서버로 `localhost:3000` 접속.
  - `?ym=2026-02` 진입 시 가구 합계 카드 상단에 수입 도넛(가운데 "수입 ₩1,120,588" 라벨), 그 아래 지출 도넛 + 범례.
  - 엉아 카드 수입 라벨 "수입 ₩5,950,643".
  - 아기 카드 income 0 이면 수입 도넛 영역 미렌더.
  - 두 도넛 동일 크기.
  - 수입 도넛 아래 카테고리 금액·% 범례 없음.

### Step 5.4: CLAUDE.md Cumulative Context 추가

- [ ] **Step 5.4:** `CLAUDE.md` 의 `## Cumulative Context` 섹션 끝에 다음 항목 추가:

```markdown
- 2026-05-08: 액터 카드 수입 도넛 — 헤더 아래 "수입 ₩X" 텍스트 줄을 동일 크기(`h-44`) 수입 도넛으로 교체. 카테고리 범례 없이 차트 + 가운데 "수입 ₩X" 라벨만. 색은 EXPENSE_PALETTE 재사용. 백엔드 `IncomeResponse.categories` 추가(additive, expense `CategorySummary` 셰이프 재사용, 부호 그대로 양수). 프론트 `buildActorIncomeSlices` / `buildHouseholdIncomeSlices` 추가, `ActorDonut` props `data/income(number)` → `expense/income(ActorDonutData)` 로 변경, 내부 `DonutChart` 헬퍼로 income/expense 두 도넛 공통화. 백엔드 +2 테스트(`income_response_includes_categories_breakdown`, `income_categories_exclude_expense_kind`), 프론트 +9 테스트(donut-data 7, dashboard ActorDonut 신규/갱신). Spec/plan: `docs/superpowers/{specs,plans}/2026-05-08-actor-income-donut*`.
```

### Step 5.5: 커밋

- [ ] **Step 5.5: 커밋**

```bash
git add CLAUDE.md
git commit -m "docs: log actor income donut changes in CLAUDE.md cumulative context"
```

---

## Self-Review

**1. Spec coverage:**
- §1 백엔드 IncomeResponse.categories — Task 1 ✓
- §2 프론트 schemas + donut-data 빌더 — Task 2 ✓
- §3 ActorDonut props 변경 + 수입 도넛 + 동일 크기 + 범례 없음 — Task 3 ✓
- §3 DashboardDonuts 갱신 — Task 4 ✓
- §4 테스트 — Task 1, 2, 3, 4 의 각 Step ✓
- §5 마이그레이션 없음 — 명시 ✓
- 수용 기준 1–6 — Task 5.3 수동 확인 + Task 5.1–2 자동 테스트 ✓

**2. Placeholder scan:** "TBD"/"TODO"/"appropriate"/"similar to" 없음 ✓

**3. Type consistency:**
- `ActorDonutData` 형태 일관(`actorId`, `actorName`, `total`, `slices`).
- `ActorDonut` props: `expense: ActorDonutData; income: ActorDonutData` — Task 3, Task 4 일치.
- testids: `donut-income-chart`, `donut-income-center`, `donut-center` (지출), `donut-empty`, `donut-no-expense` 일관 사용.
- backend: `crate::api::summary::{ByActorEntry, CategorySummary}` — 이미 `pub` 으로 노출되어 있음(검증 완료).
