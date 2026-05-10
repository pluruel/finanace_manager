# Dashboard Income/Expense Redesign — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 대시보드에서 수입(빨강 헤더)과 지출(파랑 도넛)을 시각적으로 분리하고, 차감을 별도 도넛 카드로 빼내며, 도넛 카드 순서를 가구합계/아기/엉아로 고정. 신규 카테고리 income 자동 분류 추가.

**Architecture:** 백엔드 변경 1군데(`pipeline.rs::upsert_category`)로 import 시점에 카테고리 이름 휴리스틱(`급여|수입|회수|환급`)으로 `kind='income'` 자동 부여. 프런트는 `donut-data.ts`에 4개 순수 함수(actor 슬라이스 / household 합산 / 차감 actor 슬라이스 / 액터별 income lookup) + 색상 토큰 재정의로 데이터 레이어 분리. 도넛 컴포넌트는 income 헤더 + expense 도넛 듀얼-룰 카드와 신규 차감 카드로 분리.

**Tech Stack:** Rust + sqlx (server), Next.js 15 + React + recharts + vitest (web).

**Spec:** `docs/superpowers/specs/2026-05-08-dashboard-income-expense-redesign-design.md`

---

## Task 1: 백엔드 — Importer income-keyword 휴리스틱

신규 카테고리 생성 시 정규화된 이름이 `급여|수입|회수|환급` 중 하나라도 포함하면 `kind='income'`, 그 외엔 `kind='expense'`. ON CONFLICT DO NOTHING 은 그대로라 기존 row 의 kind 는 보존됨.

**Files:**
- Create: `server/tests/test_import_kind_heuristic.rs`
- Modify: `server/src/import/pipeline.rs` (lines 44-60 in `upsert_category`)

- [ ] **Step 1: 새 테스트 파일 작성 — 골든 import 후 카테고리 kind 검증**

`server/tests/test_import_kind_heuristic.rs` 생성:

```rust
//! Importer kind 휴리스틱 통합 테스트.
//!
//! 골든 xlsx (`2026년 02월.xlsx`) 를 import 하여 신규 생성된 카테고리들의
//! `kind` 가 이름 기반 휴리스틱(`급여|수입|회수|환급`)에 따라 income/expense
//! 로 분류되는지 확인한다.

use finance_manager::import::pipeline::run_pipeline;
use finance_manager::import::xlsx::{extract_sheet_name, extract_year_month, parse_xlsx};
use sqlx::PgPool;
use uuid::Uuid;

fn load_golden_bytes() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/2026년_02월.xlsx"
    );
    std::fs::read(path).expect("Failed to read golden xlsx fixture")
}

async fn run_golden_import(pool: &PgPool, owner_id: Uuid) -> anyhow::Result<()> {
    use sha2::{Digest, Sha256};
    let bytes = load_golden_bytes();
    let filename = "2026년 02월.xlsx";
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let hash_vec = hasher.finalize().to_vec();

    let (year, month) = extract_year_month(filename).unwrap();
    let sheet_name = extract_sheet_name(filename).unwrap();
    let raw_rows = parse_xlsx(&bytes, &sheet_name)?;
    let row_count = raw_rows.len() as i32;

    let mut tx = pool.begin().await?;
    let batch_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO import_batches (owner_id, file_name, file_hash, year, month, row_count)
           VALUES ($1, $2, $3, $4, $5, $6)
           RETURNING id"#,
        owner_id, filename, hash_vec, year, month, row_count,
    )
    .fetch_one(&mut *tx)
    .await?;

    run_pipeline(&mut *tx, owner_id, batch_id, raw_rows).await?;
    tx.commit().await?;
    Ok(())
}

async fn kind_of(pool: &PgPool, owner_id: Uuid, name: &str) -> Option<String> {
    sqlx::query_scalar!(
        "SELECT kind FROM categories WHERE owner_id = $1 AND name = $2 AND parent_id IS NULL",
        owner_id, name
    )
    .fetch_optional(pool)
    .await
    .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn import_classifies_income_categories_by_name(pool: PgPool) {
    let owner_id = Uuid::new_v4();
    run_golden_import(&pool, owner_id).await.unwrap();

    // 키워드 매치 → income
    assert_eq!(kind_of(&pool, owner_id, "급여").await.as_deref(), Some("income"));
    assert_eq!(kind_of(&pool, owner_id, "회수").await.as_deref(), Some("income"));
    assert_eq!(kind_of(&pool, owner_id, "수입 기타").await.as_deref(), Some("income"));
}

#[sqlx::test(migrations = "./migrations")]
async fn import_keeps_other_categories_as_expense(pool: PgPool) {
    let owner_id = Uuid::new_v4();
    run_golden_import(&pool, owner_id).await.unwrap();

    // 키워드 미매치 → expense
    assert_eq!(kind_of(&pool, owner_id, "차감").await.as_deref(), Some("expense"));
    assert_eq!(kind_of(&pool, owner_id, "외식 아침").await.as_deref(), Some("expense"));
    assert_eq!(kind_of(&pool, owner_id, "병원").await.as_deref(), Some("expense"));
}

#[sqlx::test(migrations = "./migrations")]
async fn upsert_preserves_existing_kind_via_on_conflict(pool: PgPool) {
    // ON CONFLICT DO NOTHING 의 보존성을 SQL 레벨에서 직접 확인.
    // 이미 kind='income' 인 row 가 있을 때 동일 (owner_id, name) 으로 INSERT 시도해도
    // kind 가 'expense' 로 덮이지 않아야 한다.
    let owner_id = Uuid::new_v4();

    sqlx::query!(
        "INSERT INTO categories (owner_id, name, kind) VALUES ($1, '외식', 'income')",
        owner_id
    )
    .execute(&pool)
    .await
    .unwrap();

    let _ = sqlx::query!(
        r#"INSERT INTO categories (owner_id, name, kind, review_state)
           VALUES ($1, '외식', 'expense', 'pending')
           ON CONFLICT (owner_id, name) WHERE parent_id IS NULL DO NOTHING"#,
        owner_id
    )
    .execute(&pool)
    .await
    .unwrap();

    let kind: String = sqlx::query_scalar!(
        "SELECT kind FROM categories WHERE owner_id = $1 AND name = '외식'",
        owner_id
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(kind, "income");
}
```

- [ ] **Step 2: 테스트 실패 확인 (휴리스틱 미구현)**

Run: `cd server && cargo test -p server --test test_import_kind_heuristic`
Expected: 처음 두 테스트 실패 (`급여` 등이 'expense' 로 분류됨). 세 번째는 SQL 불변성 테스트라 통과.

- [ ] **Step 3: pipeline.rs 의 `upsert_category` 에 휴리스틱 추가**

`server/src/import/pipeline.rs` 의 라인 44-60 (위 "차감" 처리 직후 ~ INSERT 까지) 을 다음으로 교체:

```rust
    // "차감" (normalized) always gets review_state='confirmed' on first creation.
    let is_deduction = norm == "차감";
    let review_state = if is_deduction { "confirmed" } else { "pending" };

    // 카테고리 이름 휴리스틱: 정규화된 이름에 income 키워드 포함 시 'income', 그 외 'expense'.
    // ON CONFLICT DO NOTHING 으로 기존 row 의 kind 는 보존됨 (사용자 토글 / 잘못된 휴리스틱
    // 모두 한 번 결정되면 유지). 휴리스틱은 보조이지 정답이 아님 — 실데이터에서 false positive 가
    // 발견되면 /aliases Categories 탭에서 토글하면 영구 보존됨.
    const INCOME_KEYWORDS: &[&str] = &["급여", "수입", "회수", "환급"];
    let kind = if INCOME_KEYWORDS.iter().any(|kw| norm.contains(kw)) {
        "income"
    } else {
        "expense"
    };

    // 2. INSERT targeting the partial index; fallback SELECT on conflict.
    let cat_id_opt: Option<Uuid> = sqlx::query_scalar!(
        r#"INSERT INTO categories (owner_id, name, kind, review_state)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT (owner_id, name) WHERE parent_id IS NULL DO NOTHING
           RETURNING id"#,
        owner_id,
        norm,
        kind,
        review_state,
    )
    .fetch_optional(&mut *conn)
    .await
    .context("category INSERT failed")?;
```

- [ ] **Step 4: 테스트 통과 확인**

Run: `cd server && cargo test -p server --test test_import_kind_heuristic`
Expected: 세 테스트 모두 PASS.

- [ ] **Step 5: 전체 백엔드 테스트 실행 (회귀 확인)**

Run: `cd server && cargo test -p server`
Expected: 모든 기존 테스트 + 신규 3개 PASS.

- [ ] **Step 6: 커밋**

```bash
git add server/src/import/pipeline.rs server/tests/test_import_kind_heuristic.rs
git commit -m "feat(server): import-time income kind heuristic for category names

급여|수입|회수|환급 키워드 매치 시 신규 카테고리 kind='income' 으로 생성.
ON CONFLICT DO NOTHING 으로 기존 row 의 kind 는 보존됨 (사용자 토글 / 잘못된
휴리스틱 모두 첫 결정 후 유지)."
```

---

## Task 2: 프런트 — `donut-data.ts` 4함수 + 새 색상 토큰

`buildActorSlices`에서 차감 처리를 제거(슬라이스에서 제외)하고, `buildHouseholdSlices` (모든 액터 expense 카테고리별 합산), `buildDeductionByActor` (차감 카테고리만 액터별 슬라이스), `incomeFor` (액터별 / 가구 합계 income lookup) 추가. 색상 팔레트를 blue 계열로 교체하고 INCOME_COLOR / DEDUCTION_PALETTE 추가.

**Files:**
- Modify: `web/lib/donut-data.ts`
- Modify: `web/__tests__/donut-data.test.ts`

- [ ] **Step 1: 기존 테스트 갱신 + 신규 테스트 작성 (실패 상태)**

`web/__tests__/donut-data.test.ts` 를 다음으로 통째 교체:

```ts
import { describe, it, expect } from "vitest";
import {
  buildActorSlices,
  buildHouseholdSlices,
  buildDeductionByActor,
  incomeFor,
  collectOrderedActorIds,
  EXPENSE_PALETTE,
  OTHER_COLOR,
  DEDUCTION_PALETTE,
} from "../lib/donut-data";
import type { SummaryResponse, IncomeResponse } from "../lib/schemas";

const ACTOR_A = "00000000-0000-0000-0000-0000000000aa";
const ACTOR_B = "00000000-0000-0000-0000-0000000000bb";

function makeData(
  categories: Array<{ name: string; cells: Array<{ actor: string; amount: string }> }>,
): SummaryResponse {
  return {
    year: 2026,
    month: 2,
    actors: [
      { actor_id: ACTOR_A, actor_name: "공동" },
      { actor_id: ACTOR_B, actor_name: "엉아" },
    ],
    categories: categories.map((c, i) => ({
      category_id: `${"1".repeat(8)}-1111-1111-1111-${String(i).padStart(12, "0")}`,
      category_name: c.name,
      kind: "expense",
      by_actor: c.cells.map((cell) => ({
        actor_id: cell.actor,
        actor_name: cell.actor === ACTOR_A ? "공동" : "엉아",
        amount: cell.amount,
      })),
      total: "0",
    })),
  };
}

describe("buildActorSlices (지출 전용)", () => {
  it("차감 카테고리는 슬라이스에서 제외된다", () => {
    const data = makeData([
      { name: "외식", cells: [{ actor: ACTOR_A, amount: "1000" }] },
      { name: "차감", cells: [{ actor: ACTOR_A, amount: "9999" }] },
    ]);
    const result = buildActorSlices(data, ACTOR_A);
    expect(result.slices.map((s) => s.name)).toEqual(["외식"]);
    expect(result.total).toBe(1000);
  });

  it("expense 색상 팔레트(EXPENSE_PALETTE)를 사용한다", () => {
    const data = makeData([
      { name: "외식", cells: [{ actor: ACTOR_A, amount: "1000" }] },
    ]);
    const result = buildActorSlices(data, ACTOR_A);
    expect(result.slices[0].color).toBe(EXPENSE_PALETTE[0]);
  });

  it("7개 비차감 카테고리 → top-6 + 기타", () => {
    const data = makeData([
      { name: "c1", cells: [{ actor: ACTOR_A, amount: "1000" }] },
      { name: "c2", cells: [{ actor: ACTOR_A, amount: "2000" }] },
      { name: "c3", cells: [{ actor: ACTOR_A, amount: "3000" }] },
      { name: "c4", cells: [{ actor: ACTOR_A, amount: "4000" }] },
      { name: "c5", cells: [{ actor: ACTOR_A, amount: "5000" }] },
      { name: "c6", cells: [{ actor: ACTOR_A, amount: "6000" }] },
      { name: "c7", cells: [{ actor: ACTOR_A, amount: "700" }] },
    ]);
    const result = buildActorSlices(data, ACTOR_A);
    expect(result.slices.map((s) => s.name)).toEqual([
      "c6", "c5", "c4", "c3", "c2", "c1", "기타",
    ]);
    expect(result.slices[6].color).toBe(OTHER_COLOR);
  });

  it("환불(음수) 슬라이스는 보존되지만 합계를 깎는다", () => {
    const data = makeData([
      { name: "외식", cells: [{ actor: ACTOR_A, amount: "1000" }] },
      { name: "환불", cells: [{ actor: ACTOR_A, amount: "-300" }] },
    ]);
    const result = buildActorSlices(data, ACTOR_A);
    expect(result.total).toBe(700);
    expect(result.slices.map((s) => s.name).sort()).toEqual(["환불", "외식"].sort());
  });

  it("액터에 expense 행이 없으면 빈 슬라이스 + 0 total", () => {
    const data = makeData([
      { name: "외식", cells: [{ actor: ACTOR_B, amount: "100" }] },
    ]);
    const result = buildActorSlices(data, ACTOR_A);
    expect(result.slices).toEqual([]);
    expect(result.total).toBe(0);
  });
});

describe("buildHouseholdSlices", () => {
  it("모든 액터의 expense 를 카테고리별로 합산한다 (차감 제외)", () => {
    const data = makeData([
      { name: "외식", cells: [
        { actor: ACTOR_A, amount: "1000" },
        { actor: ACTOR_B, amount: "2000" },
      ]},
      { name: "병원", cells: [
        { actor: ACTOR_A, amount: "500" },
      ]},
      { name: "차감", cells: [
        { actor: ACTOR_A, amount: "200" },
      ]},
    ]);
    const result = buildHouseholdSlices(data);
    const map = new Map(result.slices.map((s) => [s.name, s.value]));
    expect(map.get("외식")).toBe(3000);
    expect(map.get("병원")).toBe(500);
    expect(map.has("차감")).toBe(false);
    expect(result.actorName).toBe("가구 합계");
    expect(result.total).toBe(3500);
  });

  it("동일 카테고리의 actor 합산 후 top-6 + 기타 적용", () => {
    const data = makeData([
      { name: "c1", cells: [{ actor: ACTOR_A, amount: "1000" }, { actor: ACTOR_B, amount: "100" }] },
      { name: "c2", cells: [{ actor: ACTOR_A, amount: "2000" }] },
      { name: "c3", cells: [{ actor: ACTOR_A, amount: "3000" }] },
      { name: "c4", cells: [{ actor: ACTOR_A, amount: "4000" }] },
      { name: "c5", cells: [{ actor: ACTOR_A, amount: "5000" }] },
      { name: "c6", cells: [{ actor: ACTOR_A, amount: "6000" }] },
      { name: "c7", cells: [{ actor: ACTOR_A, amount: "700" }] },
    ]);
    const result = buildHouseholdSlices(data);
    const names = result.slices.map((s) => s.name);
    expect(names[0]).toBe("c6"); // 6000 (가장 큼)
    expect(names).toContain("기타");
    expect(names.length).toBe(7); // top-6 + 기타
  });
});

describe("buildDeductionByActor", () => {
  it("차감 카테고리를 액터별 슬라이스로 분해한다", () => {
    const data = makeData([
      { name: "외식", cells: [{ actor: ACTOR_A, amount: "1000" }] },
      { name: "차감", cells: [
        { actor: ACTOR_A, amount: "300" },
        { actor: ACTOR_B, amount: "200" },
      ]},
    ]);
    const result = buildDeductionByActor(data);
    expect(result.slices.length).toBe(2);
    const map = new Map(result.slices.map((s) => [s.name, s.value]));
    expect(map.get("공동")).toBe(300);
    expect(map.get("엉아")).toBe(200);
    expect(result.total).toBe(500);
    // 회색조 팔레트 사용
    for (const s of result.slices) {
      expect(DEDUCTION_PALETTE).toContain(s.color);
    }
  });

  it("차감 0 인 액터는 슬라이스에서 제외", () => {
    const data = makeData([
      { name: "차감", cells: [{ actor: ACTOR_A, amount: "300" }] },
    ]);
    const result = buildDeductionByActor(data);
    expect(result.slices.map((s) => s.name)).toEqual(["공동"]);
  });

  it("차감 카테고리 자체가 없으면 빈 결과", () => {
    const data = makeData([
      { name: "외식", cells: [{ actor: ACTOR_A, amount: "1000" }] },
    ]);
    const result = buildDeductionByActor(data);
    expect(result.slices).toEqual([]);
    expect(result.total).toBe(0);
  });
});

describe("incomeFor", () => {
  const income: IncomeResponse = {
    month: "2026-02",
    by_actor: [
      { actor_id: ACTOR_A, actor_name: "공동", total: "0" },
      { actor_id: ACTOR_B, actor_name: "엉아", total: "1000" },
    ],
    total: "1000",
  };

  it("'household' 키워드는 전체 합계를 반환한다", () => {
    expect(incomeFor(income, "household")).toBe(1000);
  });

  it("actor_id 매치 시 해당 액터 income 반환", () => {
    expect(incomeFor(income, ACTOR_B)).toBe(1000);
    expect(incomeFor(income, ACTOR_A)).toBe(0);
  });

  it("매치 없으면 0 반환", () => {
    expect(incomeFor(income, "nonexistent-id")).toBe(0);
  });

  it("income 데이터가 null 이면 0 반환", () => {
    expect(incomeFor(null, "household")).toBe(0);
    expect(incomeFor(null, ACTOR_A)).toBe(0);
  });
});

describe("collectOrderedActorIds", () => {
  const A = "00000000-0000-0000-0000-0000000000aa";
  const B = "00000000-0000-0000-0000-0000000000bb";

  it("returns empty array when data is null", () => {
    expect(collectOrderedActorIds(null)).toEqual([]);
  });

  it("preserves data.actors order", () => {
    const data: SummaryResponse = {
      year: 2026,
      month: 2,
      actors: [
        { actor_id: A, actor_name: "공동" },
        { actor_id: B, actor_name: "엉아" },
      ],
      categories: [],
    };
    expect(collectOrderedActorIds(data)).toEqual([A, B]);
  });
});
```

- [ ] **Step 2: 테스트 실패 확인**

Run: `cd web && npm test -- donut-data.test.ts`
Expected: 새 함수/상수 import 에러로 다수 실패.

- [ ] **Step 3: `web/lib/donut-data.ts` 를 다음으로 교체**

```ts
import type { SummaryResponse, IncomeResponse } from "./schemas";

export type DonutSlice = {
  name: string;
  value: number;
  color: string;
  isDeduction: boolean;
  isOther: boolean;
};

export type ActorDonutData = {
  actorId: string | null;
  actorName: string;
  total: number;
  slices: DonutSlice[];
};

// blue/cyan/indigo 톤. 도넛 전체가 "지출 = 파랑" 으로 읽히게 하면서 슬라이스 구분도 가능.
export const EXPENSE_PALETTE = [
  "#1e40af", // blue-800
  "#2563eb", // blue-600
  "#3b82f6", // blue-500
  "#0891b2", // cyan-600
  "#0e7490", // cyan-700
  "#6366f1", // indigo-500
] as const;

export const OTHER_COLOR = "#94a3b8"; // slate-400
export const INCOME_COLOR = "#dc2626"; // red-600 (헤더 텍스트)
export const DEDUCTION_PALETTE = [
  "#4b5563", // gray-600
  "#6b7280", // gray-500
  "#9ca3af", // gray-400
  "#d1d5db", // gray-300
] as const;

const TOP_N = 6;
const DEDUCTION_NAME = "차감";
const OTHER_NAME = "기타";
const HOUSEHOLD_NAME = "가구 합계";

function actorNameFor(data: SummaryResponse, actorId: string | null): string {
  const fromActors = data.actors.find((a) => a.actor_id === actorId);
  if (fromActors) return fromActors.actor_name;
  for (const cat of data.categories) {
    const cell = cat.by_actor.find((e) => e.actor_id === actorId);
    if (cell) return cell.actor_name;
  }
  return actorId ?? "미지정";
}

type ExpenseRaw = { name: string; value: number };

function topNWithOther(rest: ExpenseRaw[]): DonutSlice[] {
  const sorted = [...rest].sort((a, b) => Math.abs(b.value) - Math.abs(a.value));
  const top = sorted.slice(0, TOP_N);
  const tail = sorted.slice(TOP_N);

  const slices: DonutSlice[] = top.map((r, i) => ({
    name: r.name,
    value: r.value,
    color: EXPENSE_PALETTE[i % EXPENSE_PALETTE.length],
    isDeduction: false,
    isOther: false,
  }));

  if (tail.length > 0) {
    slices.push({
      name: OTHER_NAME,
      value: tail.reduce((acc, r) => acc + r.value, 0),
      color: OTHER_COLOR,
      isDeduction: false,
      isOther: true,
    });
  }
  return slices;
}

/**
 * 단일 액터의 expense 슬라이스 (차감 제외).
 */
export function buildActorSlices(
  data: SummaryResponse,
  actorId: string | null,
): ActorDonutData {
  const actorName = actorNameFor(data, actorId);
  const raws: ExpenseRaw[] = [];

  for (const cat of data.categories) {
    if (cat.category_name === DEDUCTION_NAME) continue;
    const cell = cat.by_actor.find((e) => e.actor_id === actorId);
    if (!cell) continue;
    const v = parseFloat(cell.amount);
    if (Number.isNaN(v) || v === 0) continue;
    raws.push({ name: cat.category_name, value: v });
  }

  const total = raws.reduce((acc, r) => acc + r.value, 0);
  const slices = topNWithOther(raws);
  return { actorId, actorName, total, slices };
}

/**
 * 가구 전체 합계 — 모든 액터의 expense 를 카테고리별로 합산 (차감 제외).
 */
export function buildHouseholdSlices(data: SummaryResponse): ActorDonutData {
  const sums = new Map<string, number>();
  for (const cat of data.categories) {
    if (cat.category_name === DEDUCTION_NAME) continue;
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

/**
 * 차감 카테고리만 액터별로 분해 → 액터당 1 슬라이스.
 * 0 인 액터는 슬라이스에서 제외. 회색조 팔레트.
 */
export function buildDeductionByActor(data: SummaryResponse): ActorDonutData {
  const deductionCat = data.categories.find((c) => c.category_name === DEDUCTION_NAME);
  if (!deductionCat) {
    return { actorId: null, actorName: DEDUCTION_NAME, total: 0, slices: [] };
  }

  type Row = { actorName: string; value: number };
  const rows: Row[] = [];
  for (const cell of deductionCat.by_actor) {
    const v = parseFloat(cell.amount);
    if (Number.isNaN(v) || v === 0) continue;
    rows.push({ actorName: cell.actor_name, value: v });
  }

  rows.sort((a, b) => Math.abs(b.value) - Math.abs(a.value));

  const slices: DonutSlice[] = rows.map((r, i) => ({
    name: r.actorName,
    value: r.value,
    color: DEDUCTION_PALETTE[i % DEDUCTION_PALETTE.length],
    isDeduction: true,
    isOther: false,
  }));

  const total = rows.reduce((acc, r) => acc + r.value, 0);
  return { actorId: null, actorName: DEDUCTION_NAME, total, slices };
}

/**
 * income lookup. actorRef 가 "household" 면 전체 합계, 그 외엔 actor_id 매치.
 */
export function incomeFor(
  income: IncomeResponse | null,
  actorRef: string | "household" | null,
): number {
  if (!income) return 0;
  if (actorRef === "household") {
    const v = parseFloat(income.total);
    return Number.isNaN(v) ? 0 : v;
  }
  const row = income.by_actor.find((e) => e.actor_id === actorRef);
  if (!row) return 0;
  const v = parseFloat(row.total);
  return Number.isNaN(v) ? 0 : v;
}

export function collectOrderedActorIds(
  data: SummaryResponse | null,
): Array<string | null> {
  if (!data) return [];
  const seen = new Set<string | null>();
  const ordered: Array<string | null> = [];
  for (const a of data.actors) {
    if (!seen.has(a.actor_id)) {
      seen.add(a.actor_id);
      ordered.push(a.actor_id);
    }
  }
  for (const cat of data.categories) {
    for (const cell of cat.by_actor) {
      if (!seen.has(cell.actor_id)) {
        seen.add(cell.actor_id);
        ordered.push(cell.actor_id);
      }
    }
  }
  return ordered;
}
```

- [ ] **Step 4: 테스트 통과 확인**

Run: `cd web && npm test -- donut-data.test.ts`
Expected: 모든 케이스 PASS.

- [ ] **Step 5: 커밋**

```bash
git add web/lib/donut-data.ts web/__tests__/donut-data.test.ts
git commit -m "refactor(web): split donut-data into expense/household/deduction/income builders

- buildActorSlices 가 차감을 슬라이스에서 제외
- buildHouseholdSlices: 모든 actor 의 expense 카테고리별 합산
- buildDeductionByActor: 차감 카테고리만 액터별 슬라이스 (회색조 팔레트)
- incomeFor: 액터별/가구 income lookup 헬퍼
- 색상 토큰: EXPENSE_PALETTE (blue), DEDUCTION_PALETTE (gray), INCOME_COLOR (red)"
```

---

## Task 3: `actor-donut.tsx` — 수입 헤더 + 지출 라벨 + 새 퍼센티지

차감 special-case 제거(이미 슬라이스에 없음). 빨간 수입 헤더 행 추가(0 이면 비표시). 중앙 라벨 "지출 ₩X". 퍼센티지 분모를 `Σ|slice.value|` 로 변경. props 에 `income: number` 추가. 빈 카드 분기를 `income === 0 && slices.length === 0` 으로.

**Files:**
- Modify: `web/components/actor-donut.tsx`
- Modify: `web/__tests__/dashboard.test.tsx` (ActorDonut describe 블록만)

- [ ] **Step 1: dashboard.test.tsx 의 ActorDonut 블록을 새 props 에 맞춰 갱신**

`web/__tests__/dashboard.test.tsx` 라인 123-170 의 `describe("ActorDonut", ...)` 블록을 다음으로 교체:

```tsx
describe("ActorDonut", () => {
  const ACTOR_A = "00000000-0000-0000-0000-0000000000aa";

  it("수입 = 0 && 슬라이스 0 일 때 빈 카드 placeholder 만 렌더", () => {
    const data: SummaryResponse = {
      year: 2026,
      month: 2,
      actors: [{ actor_id: ACTOR_A, actor_name: "공동" }],
      categories: [],
    };
    render(<ActorDonut data={buildActorSlices(data, ACTOR_A)} income={0} />);
    expect(screen.getByTestId("actor-donut-empty")).toBeTruthy();
  });

  it("수입 > 0 이면 빨간색 수입 헤더 행을 렌더한다", () => {
    const data: SummaryResponse = {
      year: 2026,
      month: 2,
      actors: [{ actor_id: ACTOR_A, actor_name: "공동" }],
      categories: [],
    };
    render(<ActorDonut data={buildActorSlices(data, ACTOR_A)} income={5741025} />);
    const header = screen.getByTestId("actor-donut-income");
    expect(header.textContent).toContain("수입");
    expect(header.textContent).toContain("5,741,025");
  });

  it("수입 = 0 일 때 수입 헤더 행은 미렌더", () => {
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
    render(<ActorDonut data={buildActorSlices(data, ACTOR_A)} income={0} />);
    expect(screen.queryByTestId("actor-donut-income")).toBeNull();
  });

  it("중앙 라벨이 '지출' + 합계를 표시한다", () => {
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
    render(<ActorDonut data={buildActorSlices(data, ACTOR_A)} income={0} />);
    const center = screen.getByTestId("actor-donut-center");
    expect(center.textContent).toContain("지출");
    expect(center.textContent).toContain("100,000");
  });

  it("퍼센티지 분모는 Σ|value| (100% 수렴)", () => {
    // 두 슬라이스 1000 + 3000 → 25% / 75%
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
    render(<ActorDonut data={buildActorSlices(data, ACTOR_A)} income={0} />);
    expect(screen.getByText(/병원/).parentElement?.parentElement?.textContent).toContain("75.0%");
    expect(screen.getByText(/외식/).parentElement?.parentElement?.textContent).toContain("25.0%");
  });

  it("수입 > 0 + 슬라이스 0 인 액터는 placeholder 텍스트 + 헤더만", () => {
    const data: SummaryResponse = {
      year: 2026,
      month: 2,
      actors: [{ actor_id: ACTOR_A, actor_name: "공동" }],
      categories: [],
    };
    render(<ActorDonut data={buildActorSlices(data, ACTOR_A)} income={1000} />);
    expect(screen.getByTestId("actor-donut-income")).toBeTruthy();
    expect(screen.getByTestId("actor-donut-no-expense")).toBeTruthy();
    expect(screen.queryByTestId("actor-donut-empty")).toBeNull();
  });
});
```

- [ ] **Step 2: 테스트 실패 확인**

Run: `cd web && npm test -- dashboard.test.tsx`
Expected: ActorDonut describe 의 새 케이스들이 prop 시그니처 변경(`income`)으로 실패.

- [ ] **Step 3: `web/components/actor-donut.tsx` 를 다음으로 교체**

```tsx
"use client";

import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { PieChart, Pie, Cell, ResponsiveContainer, Tooltip } from "recharts";
import type { ActorDonutData, DonutSlice } from "@/lib/donut-data";
import { INCOME_COLOR } from "@/lib/donut-data";

type Props = {
  data: ActorDonutData;
  income: number;
};

function fmtSigned(v: number): string {
  const abs = Math.abs(v).toLocaleString("ko-KR");
  return v < 0 ? `-₩${abs}` : `₩${abs}`;
}

function pctOfAbs(value: number, slices: DonutSlice[]): string {
  const denom = slices.reduce((acc, s) => acc + Math.abs(s.value), 0);
  if (denom === 0) return "0%";
  return `${((Math.abs(value) / denom) * 100).toFixed(1)}%`;
}

export function ActorDonut({ data, income }: Props) {
  const { actorName, total, slices } = data;
  const hasIncome = income > 0;
  const hasSlices = slices.length > 0;
  const hasNothing = !hasIncome && !hasSlices;

  return (
    <Card data-testid={`actor-donut-${actorName}`}>
      <CardHeader>
        <CardTitle className="text-base">{actorName}</CardTitle>
      </CardHeader>
      <CardContent>
        {hasNothing ? (
          <p className="text-sm text-muted-foreground" data-testid="actor-donut-empty">
            이 달의 거래 내역이 없습니다.
          </p>
        ) : (
          <div className="flex flex-col gap-3">
            {hasIncome && (
              <div
                data-testid="actor-donut-income"
                className="flex items-center justify-between text-sm"
              >
                <span className="font-medium" style={{ color: INCOME_COLOR }}>
                  수입
                </span>
                <span
                  className="font-mono font-semibold tabular-nums"
                  style={{ color: INCOME_COLOR }}
                >
                  {fmtSigned(income)}
                </span>
              </div>
            )}

            {hasSlices ? (
              <>
                <div className="relative h-44">
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
                    data-testid="actor-donut-center"
                    className="absolute inset-0 pointer-events-none flex flex-col items-center justify-center"
                  >
                    <span className="text-xs text-muted-foreground">지출</span>
                    <span className="text-base font-semibold tabular-nums">
                      {fmtSigned(total)}
                    </span>
                  </div>
                </div>
                <ul className="text-sm space-y-1">
                  {slices.map((s: DonutSlice, i) => (
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
                        {fmtSigned(s.value)} · {pctOfAbs(s.value, slices)}
                      </span>
                    </li>
                  ))}
                </ul>
              </>
            ) : (
              <p
                data-testid="actor-donut-no-expense"
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

- [ ] **Step 4: 테스트 통과 확인**

Run: `cd web && npm test -- dashboard.test.tsx`
Expected: ActorDonut describe 의 6 케이스 모두 PASS. 다른 describe (DashboardDonuts 등) 는 일부 실패 가능 — 다음 태스크에서 처리.

- [ ] **Step 5: 커밋**

```bash
git add web/components/actor-donut.tsx web/__tests__/dashboard.test.tsx
git commit -m "feat(web): actor donut card with red income header and expense-only donut

- 빨간 수입 헤더 행 (income > 0 일 때만)
- 중앙 라벨 '지출 ₩X'
- 퍼센티지 분모 Σ|slice.value| (sign-safe, 100% 수렴)
- 차감 special-case 제거 (별도 카드로 분리됨)"
```

---

## Task 4: `dashboard-donuts.tsx` (3-card 고정 순서) + 신규 `deduction-donut.tsx`

`DashboardDonuts` 가 `{ summary, income }` 둘 다 받아 가구합계/아기/엉아 3장을 고정 순서로 렌더. 액터 lookup 은 `actor_name === "아기" | "엉아"` 매칭. 새 `DeductionDonut` 컴포넌트가 차감 카드를 렌더(슬라이스 0 → null).

**Files:**
- Modify: `web/components/dashboard-donuts.tsx`
- Create: `web/components/deduction-donut.tsx`
- Modify: `web/__tests__/dashboard.test.tsx` (DashboardDonuts describe + 신규 DeductionDonut describe)

- [ ] **Step 1: dashboard.test.tsx 의 DashboardDonuts 블록 갱신 + DeductionDonut 신규 describe 추가**

기존 `describe("DashboardDonuts", ...)` (라인 189 이하) 를 다음으로 교체. 또 IncomeStrip describe 블록 (라인 172-187) 은 함께 삭제. 파일 상단 import 도 `IncomeStrip` 줄을 제거하고 `DeductionDonut` 추가.

```tsx
// 파일 상단 import 영역에 DeductionDonut 추가, IncomeStrip import 줄 삭제
import { DeductionDonut } from "../components/deduction-donut";

// IncomeStrip describe 전체 삭제 (라인 172-187).

describe("DashboardDonuts (가구합계 / 아기 / 엉아 3장)", () => {
  const A = "00000000-0000-0000-0000-0000000000aa";
  const BABY = "00000000-0000-0000-0000-0000000000bb";
  const SPOUSE = "00000000-0000-0000-0000-0000000000cc";

  function makeIncome(): IncomeResponse {
    return {
      month: "2026-02",
      by_actor: [
        { actor_id: A, actor_name: "공동", total: "0" },
        { actor_id: BABY, actor_name: "아기", total: "5741025" },
        { actor_id: SPOUSE, actor_name: "엉아", total: "6475664" },
      ],
      total: "12216689",
    };
  }

  function makeSummary(): SummaryResponse {
    return {
      year: 2026,
      month: 2,
      actors: [
        { actor_id: A, actor_name: "공동" },
        { actor_id: BABY, actor_name: "아기" },
        { actor_id: SPOUSE, actor_name: "엉아" },
      ],
      categories: [
        {
          category_id: "11111111-1111-1111-1111-111111111111",
          category_name: "외식",
          kind: "expense",
          by_actor: [
            { actor_id: A, actor_name: "공동", amount: "1000" },
            { actor_id: BABY, actor_name: "아기", amount: "200" },
            { actor_id: SPOUSE, actor_name: "엉아", amount: "300" },
          ],
          total: "1500",
        },
      ],
    };
  }

  it("data 가 null 이면 empty 카드", () => {
    render(<DashboardDonuts summary={null} income={null} />);
    expect(screen.getByTestId("dashboard-donuts-empty")).toBeTruthy();
  });

  it("3장의 카드를 고정 순서 가구합계 / 아기 / 엉아 로 렌더한다", () => {
    render(<DashboardDonuts summary={makeSummary()} income={makeIncome()} />);
    const cards = screen.getAllByTestId(/^actor-donut-/);
    // 첫 카드는 가구 합계, 다음은 아기, 다음은 엉아
    expect(cards[0].getAttribute("data-testid")).toBe("actor-donut-가구 합계");
    expect(cards[1].getAttribute("data-testid")).toBe("actor-donut-아기");
    expect(cards[2].getAttribute("data-testid")).toBe("actor-donut-엉아");
  });

  it("가구 합계 카드의 income 헤더는 income.total 을 사용한다", () => {
    render(<DashboardDonuts summary={makeSummary()} income={makeIncome()} />);
    const householdCard = screen.getByTestId("actor-donut-가구 합계");
    expect(householdCard.textContent).toContain("12,216,689");
  });

  it("아기 카드의 income 헤더는 by_actor 매치 값", () => {
    render(<DashboardDonuts summary={makeSummary()} income={makeIncome()} />);
    const babyCard = screen.getByTestId("actor-donut-아기");
    expect(babyCard.textContent).toContain("5,741,025");
  });

  it("income 0 인 아기 카드는 수입 헤더 미렌더", () => {
    const income: IncomeResponse = {
      month: "2026-02",
      by_actor: [
        { actor_id: BABY, actor_name: "아기", total: "0" },
        { actor_id: SPOUSE, actor_name: "엉아", total: "1000" },
      ],
      total: "1000",
    };
    render(<DashboardDonuts summary={makeSummary()} income={income} />);
    const babyCard = screen.getByTestId("actor-donut-아기");
    // 수입 헤더 testid 는 카드 내부에 없어야 함
    expect(babyCard.querySelector('[data-testid="actor-donut-income"]')).toBeNull();
  });
});

describe("DeductionDonut", () => {
  const A = "00000000-0000-0000-0000-0000000000aa";
  const B = "00000000-0000-0000-0000-0000000000bb";

  it("차감 데이터가 없으면 null 반환 (DOM 미렌더)", () => {
    const data: SummaryResponse = {
      year: 2026,
      month: 2,
      actors: [{ actor_id: A, actor_name: "공동" }],
      categories: [
        {
          category_id: "11111111-1111-1111-1111-111111111111",
          category_name: "외식",
          kind: "expense",
          by_actor: [{ actor_id: A, actor_name: "공동", amount: "1000" }],
          total: "1000",
        },
      ],
    };
    const { container } = render(<DeductionDonut summary={data} />);
    expect(container.firstChild).toBeNull();
  });

  it("차감 슬라이스를 액터 이름으로 렌더하고 중앙에 합계 표시", () => {
    const data: SummaryResponse = {
      year: 2026,
      month: 2,
      actors: [
        { actor_id: A, actor_name: "공동" },
        { actor_id: B, actor_name: "엉아" },
      ],
      categories: [
        {
          category_id: "11111111-1111-1111-1111-111111111111",
          category_name: "차감",
          kind: "expense",
          by_actor: [
            { actor_id: A, actor_name: "공동", amount: "300" },
            { actor_id: B, actor_name: "엉아", amount: "200" },
          ],
          total: "500",
        },
      ],
    };
    render(<DeductionDonut summary={data} />);
    expect(screen.getByTestId("deduction-donut")).toBeTruthy();
    expect(screen.getByText("공동")).toBeTruthy();
    expect(screen.getByText("엉아")).toBeTruthy();
    expect(screen.getByTestId("deduction-donut-center").textContent).toContain("500");
  });
});
```

- [ ] **Step 2: 테스트 실패 확인**

Run: `cd web && npm test -- dashboard.test.tsx`
Expected: `DeductionDonut` import 미존재 + `DashboardDonuts` props 시그니처 변경으로 다수 실패.

- [ ] **Step 3: `web/components/deduction-donut.tsx` 신규 작성**

```tsx
"use client";

import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { PieChart, Pie, Cell, ResponsiveContainer, Tooltip } from "recharts";
import { buildDeductionByActor } from "@/lib/donut-data";
import type { SummaryResponse } from "@/lib/schemas";

type Props = {
  summary: SummaryResponse | null;
};

function fmt(v: number): string {
  return `₩${Math.abs(v).toLocaleString("ko-KR")}`;
}

export function DeductionDonut({ summary }: Props) {
  if (!summary) return null;
  const { slices, total } = buildDeductionByActor(summary);
  if (slices.length === 0) return null;

  const denom = slices.reduce((acc, s) => acc + Math.abs(s.value), 0);

  return (
    <Card data-testid="deduction-donut">
      <CardHeader>
        <CardTitle className="text-base">차감</CardTitle>
      </CardHeader>
      <CardContent>
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4 items-center">
          <div className="relative h-44">
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
                  formatter={(v: number) => fmt(v)}
                  contentStyle={{ fontSize: 12 }}
                />
              </PieChart>
            </ResponsiveContainer>
            <div
              data-testid="deduction-donut-center"
              className="absolute inset-0 pointer-events-none flex flex-col items-center justify-center"
            >
              <span className="text-xs text-muted-foreground">차감 합계</span>
              <span className="text-base font-semibold tabular-nums">{fmt(total)}</span>
            </div>
          </div>
          <ul className="text-sm space-y-1">
            {slices.map((s, i) => (
              <li key={`${s.name}-${i}`} className="flex items-center justify-between gap-2">
                <span className="flex items-center gap-2 truncate">
                  <span
                    aria-hidden="true"
                    className="inline-block h-2.5 w-2.5 rounded-sm shrink-0"
                    style={{ backgroundColor: s.color }}
                  />
                  <span className="truncate">{s.name}</span>
                </span>
                <span className="tabular-nums text-muted-foreground shrink-0">
                  {fmt(s.value)}
                  {denom > 0 && ` · ${((Math.abs(s.value) / denom) * 100).toFixed(1)}%`}
                </span>
              </li>
            ))}
          </ul>
        </div>
      </CardContent>
    </Card>
  );
}
```

- [ ] **Step 4: `web/components/dashboard-donuts.tsx` 를 다음으로 교체**

```tsx
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { PieChart } from "lucide-react";
import { ActorDonut } from "./actor-donut";
import {
  buildActorSlices,
  buildHouseholdSlices,
  incomeFor,
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

  const householdData = buildHouseholdSlices(summary);
  const householdIncome = incomeFor(income, "household");

  const personCards = PERSON_NAMES.map((name) => {
    const actor = summary.actors.find((a) => a.actor_name === name);
    const data = actor
      ? buildActorSlices(summary, actor.actor_id)
      : { actorId: null, actorName: name, total: 0, slices: [] };
    const personIncome = actor ? incomeFor(income, actor.actor_id) : 0;
    return { data, income: personIncome };
  });

  return (
    <div className="grid grid-cols-1 md:grid-cols-3 gap-4" data-testid="dashboard-donuts">
      <ActorDonut data={householdData} income={householdIncome} />
      {personCards.map((pc) => (
        <ActorDonut key={pc.data.actorName} data={pc.data} income={pc.income} />
      ))}
    </div>
  );
}
```

- [ ] **Step 5: 테스트 통과 확인**

Run: `cd web && npm test -- dashboard.test.tsx`
Expected: DashboardDonuts + DeductionDonut describe 모두 PASS. (donut-data.test.ts 도 영향 없음.)

- [ ] **Step 6: 커밋**

```bash
git add web/components/dashboard-donuts.tsx web/components/deduction-donut.tsx web/__tests__/dashboard.test.tsx
git commit -m "feat(web): 3-card fixed-order donut grid + standalone deduction donut

- DashboardDonuts: { summary, income } props, 카드 3장 고정 순서 가구합계/아기/엉아
- 신규 DeductionDonut 컴포넌트: 차감 카테고리만 액터별 슬라이스, 회색조 팔레트
- 차감 슬라이스 0 인 달은 카드 자체 미렌더"
```

---

## Task 5: `page.tsx` 통합 + IncomeStrip 제거 + 최종 검증

페이지에서 `IncomeSection` 제거, `fetchIncome` 결과를 `DashboardDonutsSection` 으로 전달, 새 `DeductionDonutSection` 추가. `income-strip.tsx` 와 `income-strip.test.tsx` 삭제. CLAUDE.md cumulative context 갱신.

**Files:**
- Modify: `web/app/(app)/page.tsx`
- Delete: `web/components/income-strip.tsx`
- Delete: `web/__tests__/income-strip.test.tsx`
- Modify: `CLAUDE.md`

- [ ] **Step 1: `web/app/(app)/page.tsx` 를 다음으로 교체**

```tsx
import { Suspense } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { apiFetch } from "@/lib/api";
import {
  SettlementSchema,
  Settlement,
  SummaryResponseSchema,
  SummaryResponse,
  IncomeResponseSchema,
  IncomeResponse,
} from "@/lib/schemas";
import { LayoutDashboard, Download } from "lucide-react";
import { MonthPicker } from "@/components/month-picker";
import { SettlementCard } from "@/components/settlement-card";
import { DashboardDonuts } from "@/components/dashboard-donuts";
import { DeductionDonut } from "@/components/deduction-donut";

function parseYM(input: string | undefined): { year: number; month: number } {
  if (input && /^\d{4}-\d{2}$/.test(input)) {
    const [y, m] = input.split("-").map(Number);
    if (m >= 1 && m <= 12) return { year: y, month: m };
  }
  const now = new Date();
  return { year: now.getFullYear(), month: now.getMonth() + 1 };
}

async function fetchSettlement(year: number, month: number): Promise<Settlement | null> {
  try {
    return await apiFetch<Settlement>(`/api/settlement/${year}/${month}`, {
      schema: SettlementSchema,
    });
  } catch {
    return null;
  }
}

async function fetchSummary(year: number, month: number): Promise<SummaryResponse | null> {
  try {
    return await apiFetch<SummaryResponse>(`/api/summary/${year}/${month}`, {
      schema: SummaryResponseSchema,
    });
  } catch {
    return null;
  }
}

async function fetchIncome(year: number, month: number): Promise<IncomeResponse | null> {
  try {
    return await apiFetch<IncomeResponse>(`/api/summary/income/${year}/${month}`, {
      schema: IncomeResponseSchema,
    });
  } catch {
    return null;
  }
}

async function SettlementSection({ year, month }: { year: number; month: number }) {
  const data = await fetchSettlement(year, month);
  return <SettlementCard year={year} month={month} data={data} compact />;
}

async function DashboardDonutsSection({ year, month }: { year: number; month: number }) {
  const [summary, income] = await Promise.all([
    fetchSummary(year, month),
    fetchIncome(year, month),
  ]);
  return <DashboardDonuts summary={summary} income={income} />;
}

async function DeductionDonutSection({ year, month }: { year: number; month: number }) {
  const summary = await fetchSummary(year, month);
  return <DeductionDonut summary={summary} />;
}

interface PageProps {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}

export default async function DashboardPage({ searchParams }: PageProps) {
  const params = await searchParams;
  const ymRaw = typeof params.ym === "string" ? params.ym : undefined;
  const { year, month } = parseYM(ymRaw);

  const sectionKey = `${year}-${month}`;

  return (
    <div className="max-w-5xl mx-auto space-y-4">
      <div className="flex items-center justify-between gap-3">
        <div className="flex items-center gap-3">
          <LayoutDashboard className="h-6 w-6" />
          <h1 className="text-2xl font-bold">대시보드</h1>
        </div>
        <div className="flex items-center gap-2">
          <a
            href={`/api/export-proxy/${year}/${month}`}
            download
            className="inline-flex items-center gap-1.5 h-9 px-3 rounded-md border border-input bg-background text-sm font-medium hover:bg-accent hover:text-accent-foreground transition-colors"
            data-testid="export-download-link"
          >
            <Download className="h-4 w-4" />
            Excel 다운로드
          </a>
          <MonthPicker year={year} month={month} />
        </div>
      </div>

      <Suspense
        key={`settlement-${sectionKey}`}
        fallback={<StripSkeleton />}
      >
        <SettlementSection year={year} month={month} />
      </Suspense>

      <Suspense
        key={`donuts-${sectionKey}`}
        fallback={<DonutsSkeleton />}
      >
        <DashboardDonutsSection year={year} month={month} />
      </Suspense>

      <Suspense
        key={`deduction-${sectionKey}`}
        fallback={null}
      >
        <DeductionDonutSection year={year} month={month} />
      </Suspense>
    </div>
  );
}

function StripSkeleton() {
  return (
    <div className="rounded-md border bg-card px-4 py-3 animate-pulse">
      <div className="h-4 bg-muted rounded w-1/3" />
    </div>
  );
}

function DonutsSkeleton() {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">카테고리 분포</CardTitle>
      </CardHeader>
      <CardContent>
        <div className="animate-pulse space-y-2">
          <div className="h-4 bg-muted rounded w-3/4" />
          <div className="h-4 bg-muted rounded w-1/2" />
        </div>
      </CardContent>
    </Card>
  );
}
```

- [ ] **Step 2: IncomeStrip 컴포넌트와 단위 테스트 삭제**

```bash
rm web/components/income-strip.tsx
rm web/__tests__/income-strip.test.tsx
```

- [ ] **Step 3: 프런트 전체 테스트 실행**

Run: `cd web && npm test`
Expected: 모든 테스트 PASS. IncomeStrip describe 가 dashboard.test.tsx 에서 제거됐고, income-strip.test.tsx 가 삭제됐는지 확인. 새 ActorDonut/DeductionDonut/DashboardDonuts/donut-data 테스트 모두 통과.

- [ ] **Step 4: 백엔드 전체 테스트 실행**

Run: `cd server && cargo test -p server`
Expected: 모든 테스트 PASS (Task 1 의 신규 3개 + 기존 모두).

- [ ] **Step 5: CLAUDE.md cumulative context 항목 추가**

`CLAUDE.md` 의 `## Cumulative Context (Documentation Agent)` 섹션 마지막에 다음 한 줄을 append:

```markdown
- 2026-05-08: Dashboard 수입/지출 시각 분리 — 도넛 카드는 가구합계/아기/엉아 3장 고정 순서로 expense 만 표시(파란 팔레트), 수입은 카드 헤더에 빨간 텍스트로 흡수, 차감은 별도 도넛 카드로 분리(액터 슬라이스). `IncomeStrip` 컴포넌트는 제거. 백엔드는 importer 휴리스틱(`급여|수입|회수|환급`) 으로 신규 카테고리 `kind='income'` 자동 분류, ON CONFLICT DO NOTHING 으로 사용자 토글 보존. 신규/수정: `web/components/{actor-donut,dashboard-donuts,deduction-donut}.tsx`, `web/lib/donut-data.ts` (4 함수 + EXPENSE_PALETTE/DEDUCTION_PALETTE/INCOME_COLOR), `server/src/import/pipeline.rs::upsert_category`. 테스트: 백엔드 +3 (`tests/test_import_kind_heuristic.rs`), 프런트 donut-data 13개 / dashboard ActorDonut 6개 / DashboardDonuts 5개 / DeductionDonut 2개. Spec/plan: `docs/superpowers/{specs,plans}/2026-05-08-dashboard-income-expense-redesign*`.
```

- [ ] **Step 6: 커밋**

```bash
git add web/app/\(app\)/page.tsx CLAUDE.md
git rm web/components/income-strip.tsx web/__tests__/income-strip.test.tsx
git commit -m "feat(web): wire dashboard with income-in-card + deduction donut, remove IncomeStrip

- page.tsx: IncomeSection 제거, DashboardDonutsSection 에 income 데이터 전달
- 신규 DeductionDonutSection 으로 차감 도넛 렌더
- IncomeStrip 컴포넌트 / 테스트 삭제 (콘텐츠가 액터 카드 헤더로 흡수됨)
- CLAUDE.md cumulative context 갱신"
```

- [ ] **Step 7: 데이터 wipe + 골든 xlsx 재import 동작 확인 (수동)**

CLAUDE.md migration policy 에 따라 사용자가 직접 실행:

```bash
# 1. DB 초기화 (Compose 환경)
docker compose down -v && docker compose up -d postgres
cd server && sqlx migrate run

# 2. 서버/웹 기동 후 골든 xlsx 재 import
docker compose up -d server web
# 또는 로컬 dev: cargo run -p server (server) + npm run dev (web)
```

브라우저에서 `/` 열어 다음 확인:
- 가구합계 / 아기 / 엉아 3장 도넛
- 각 카드 헤더에 빨간 "수입 ₩X" (수입>0 인 액터)
- 도넛 슬라이스에 급여/회수/수입 기타 안 보임
- 퍼센티지 합 ≈ 100%
- 도넛 그리드 아래 차감 카드 1장 (액터별 슬라이스)
- 정산 카드는 그대로

---

## Self-Review Notes

**Spec coverage:**
- 백엔드 휴리스틱 (Spec §"백엔드 변경") → Task 1 ✓
- 4 함수 + 색상 토큰 (Spec §"슬라이스 빌더", §"색상 토큰") → Task 2 ✓
- 액터 카드 수입 헤더 + 지출 도넛 + 새 퍼센티지 (Spec §"지출 도넛 카드") → Task 3 ✓
- 가구합계 / 아기 / 엉아 3장 고정 (Spec §"페이지 레이아웃", §"가구 합계 카드") + 차감 카드 (Spec §"차감 도넛 카드") → Task 4 ✓
- 페이지 통합 + IncomeStrip 삭제 (Spec §"수정", §"삭제") → Task 5 ✓
- 모든 테스트 (Spec §"테스트") → Task 1/2/3/4 분산 ✓

**Type consistency:**
- `ActorDonutData` shape는 모든 빌더가 동일하게 반환 (`{actorId, actorName, total, slices}`).
- `ActorDonut` props: `{ data: ActorDonutData; income: number }` — Task 3에서 정의, Task 4 에서 일관 사용.
- `DashboardDonuts` props: `{ summary, income }` — Task 4에서 정의, Task 5 에서 일관 사용.
- `DeductionDonut` props: `{ summary }` — Task 4에서 정의, Task 5 에서 일관 사용.
- `incomeFor` 시그니처: `(income, actorRef: string | "household" | null) => number` — Task 2 정의, Task 4 사용.

**Edge case 처리:**
- 수입 0 + 지출 0 → 카드 빈 placeholder (Task 3 Step 1 첫 케이스)
- 수입 > 0 + 지출 0 → 헤더 + "이 달 지출 없음" (Task 3 Step 1 마지막 케이스)
- 차감 0 → 카드 미렌더 (Task 4 DeductionDonut 첫 케이스)
