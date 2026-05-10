# 수입/지출 분리 + 부호 규약 변경 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `transactions.sign` 컬럼을 삭제하고 `amount` 를 캐시플로우 부호(유입+, 유출−)로 저장하도록 전환한다. `categories.kind` 가 수입/지출 분류의 단일 기준이 되며, 대시보드에 액터별 수입 스트립과 `/aliases` 카테고리 탭의 수입↔지출 토글을 추가한다.

**Architecture:** 백엔드는 sqlx 컴파일 체크가 모든 쿼리를 단번에 깨므로 스키마 → 도메인/파이프라인 → 모든 read 쿼리 순으로 한 번에 갈아엎고 `cargo sqlx prepare` 로 캐시를 재생성한 뒤 빌드 검증. 그 다음 신규 엔드포인트(수입 합계, 카테고리 kind PATCH) 추가, DB wipe + 재임포트, 마지막으로 프론트엔드 스키마/컴포넌트 갱신.

**Tech Stack:** Rust (axum, sqlx, rust_decimal), PostgreSQL 17, Next.js 15 App Router, TypeScript, Zod, Vitest, Tailwind, shadcn/ui.

**Spec:** `docs/superpowers/specs/2026-05-07-income-expense-sign-design.md`

---

## 사전 점검

- [ ] **Step 0a: 사전 빌드 그린 확인**

```bash
cd server && cargo test -p server 2>&1 | tail -5
cd ../web && npm test 2>&1 | tail -5
```

기준선이 깨져 있으면 본 플랜을 진행하지 말고 먼저 main 을 고친다.

- [ ] **Step 0b: 작업 브랜치 생성 (필요 시)**

```bash
git checkout -b feat/income-expense-sign
```

---

## Task 1: DB 스키마 재작성

**Files:**
- Modify: `server/migrations/001_init.sql:120-167`

서비스 전이므로 마이그레이션을 추가하지 않고 `001_init.sql` 을 직접 재작성한다.

- [ ] **Step 1.1: `transactions` 테이블에서 `sign` 컬럼 제거**

`server/migrations/001_init.sql` 의 `CREATE TABLE transactions` 블록을 다음으로 교체한다 (라인 121–137):

```sql
-- 정규화된 거래 (대시보드/집계의 소스). 라인 단위 저장.
-- amount 는 캐시플로우 부호: 현금 유입 양수, 유출 음수.
-- 임포트 시 엑셀 라인 금액의 부호를 반전해서 저장한다 (엑셀은 지출 장부 관점이므로).
CREATE TABLE transactions (
  id                uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id          uuid NOT NULL,
  raw_id            uuid NOT NULL REFERENCES transactions_raw(id) ON DELETE CASCADE,
  group_id          uuid NOT NULL,
  occurred_on       date NOT NULL,
  merchant_id       uuid REFERENCES merchants(id),
  actor_id          uuid REFERENCES ledger_actors(id),
  category_id       uuid REFERENCES categories(id),
  product_id        uuid REFERENCES products(id),
  payment_method_id uuid REFERENCES payment_methods(id),
  amount            numeric(15,2) NOT NULL,  -- 캐시플로우 부호 (유입+/유출-)
  unit_price        numeric(15,4),
  quantity          numeric(15,4),
  memo              text
);
CREATE INDEX transactions_date_idx ON transactions (owner_id, occurred_on DESC);
CREATE INDEX transactions_category_idx ON transactions (owner_id, category_id, occurred_on);
CREATE INDEX transactions_merchant_idx ON transactions (owner_id, merchant_id, occurred_on);
CREATE INDEX transactions_product_idx ON transactions (owner_id, product_id, occurred_on);
CREATE INDEX transactions_group_idx ON transactions (owner_id, group_id);
```

- [ ] **Step 1.2: `v_monthly_settlement` 뷰 재작성**

라인 144–166 의 뷰를 다음으로 교체한다:

```sql
-- 정산 뷰: 차감을 분리한 공동 정산 산출
-- 저장 규약상 지출은 음수, 차감도 음수, 환불은 양수.
-- 정산 카드는 양수로 표시하므로 모두 -SUM(...) 로 양수화한다.
-- recognized_expense: 공동 actor 의 일반 지출(차감 제외) 양수화
-- deducted_amount: 차감 카테고리 합계(actor 무관) 양수화
-- settlement_input: recognized_expense - deducted_amount
-- 수입(kind='income')은 정산에 포함하지 않는다 (도메인 규칙).
CREATE VIEW v_monthly_settlement AS
SELECT
  t.owner_id,
  date_trunc('month', t.occurred_on)::date AS month,
  COALESCE(-SUM(t.amount) FILTER (
    WHERE actor.name = '공동' AND c.kind = 'expense' AND c.name <> '차감'
  ), 0) AS recognized_expense,
  COALESCE(-SUM(t.amount) FILTER (WHERE c.name = '차감'), 0) AS deducted_amount,
  COALESCE(-SUM(t.amount) FILTER (
    WHERE actor.name = '공동' AND c.kind = 'expense' AND c.name <> '차감'
  ), 0)
  - COALESCE(-SUM(t.amount) FILTER (WHERE c.name = '차감'), 0) AS settlement_input
FROM transactions t
JOIN categories c        ON c.id     = t.category_id
JOIN ledger_actors actor ON actor.id = t.actor_id
GROUP BY t.owner_id, date_trunc('month', t.occurred_on);
```

- [ ] **Step 1.3: 로컬 DB 재생성 + 마이그레이션 적용**

```bash
cd server
# .env 의 DATABASE_URL 이 가리키는 DB 를 drop & create
psql "$DATABASE_URL" -c "DROP SCHEMA public CASCADE; CREATE SCHEMA public;"
sqlx migrate run
```

기대: 에러 없이 `001_init.sql` 적용 완료.

- [ ] **Step 1.4: 커밋 (스키마만, 코드는 다음 태스크에서 깨졌다 살림)**

```bash
git add server/migrations/001_init.sql
git commit -m "refactor(server): drop transactions.sign and rewrite settlement view for signed amount"
```

---

## Task 2: 도메인 + 임포트 파이프라인 수정

**Files:**
- Modify: `server/src/domain/mod.rs:39`
- Modify: `server/src/import/pipeline.rs:395-422,430-450,540-602`

본 태스크 끝까지는 `cargo build` 가 깨진 채로 진행된다 (sqlx 쿼리 캐시 미스). Task 4 에서 한 번에 재컴파일.

- [ ] **Step 2.1: `TransactionRow` 에서 `sign` 필드 삭제**

`server/src/domain/mod.rs:29-43` 의 `TransactionRow` 를 다음으로 교체:

```rust
/// 정규화된 거래 행 (transactions에 저장될 형태)
#[derive(Debug, Clone)]
pub struct TransactionRow {
    pub raw_id: Uuid,
    pub group_id: Uuid,
    pub occurred_on: NaiveDate,
    pub merchant_id: Option<Uuid>,
    pub actor_id: Option<Uuid>,
    pub category_id: Option<Uuid>,
    pub product_id: Option<Uuid>,
    pub payment_method_id: Option<Uuid>,
    pub amount: Decimal,  // 캐시플로우 부호 (유입+/유출-)
    pub unit_price: Option<Decimal>,
    pub quantity: Option<Decimal>,
    pub memo: Option<String>,
}
```

- [ ] **Step 2.2: 임포트 파이프라인 — sign 분기 제거 + 부호 반전 저장**

`server/src/import/pipeline.rs:540-602` (sign 계산 + TransactionRow 생성 + insert 호출 부분) 을 다음으로 교체:

```rust
        // 4. amount 계산 — 엑셀 부호를 반전해서 캐시플로우 부호로 저장.
        //    기존 sign 컬럼은 폐기. 환불(엑셀 음수)은 저장 시 양수가 되어
        //    같은 expense 카테고리 안에서 자연 차감.
        let raw_amount = match row.line_amount.or(row.total_amount) {
            Some(a) => a,
            None => {
                tracing::warn!("Row {}: no amount, skipping", row.row_index);
                continue;
            }
        };
        let amount = -raw_amount;

        // 5. Product mapping (memo-bearing rows only).
        let product_id = if let (Some(ref memo), Some(merch_id)) = (&row.memo, merchant_id) {
            if !memo.is_empty() {
                let (id, is_new) = upsert_product(conn, owner_id, merch_id, memo).await?;
                if is_new {
                    unresolved.push(UnresolvedAlias {
                        scope: "product".to_string(),
                        raw_text: memo.clone(),
                        norm_key: to_norm_key(memo),
                    });
                }
                Some(id)
            } else {
                None
            }
        } else {
            None
        };

        // 6. Insert normalized transaction.
        let t = TransactionRow {
            raw_id,
            group_id: row.group_id,
            occurred_on,
            merchant_id,
            actor_id,
            category_id,
            product_id,
            payment_method_id,
            amount,
            unit_price: row.unit_price,
            quantity: row.quantity,
            memo: row.memo.clone(),
        };

        insert_transaction(conn, owner_id, &t).await?;
        transactions_inserted += 1;
    }
```

또한 그 위쪽의 `is_deduction` 로컬 변수가 amount 계산에서 사라지므로 (카테고리 upsert 쪽 `is_deduction` 은 그대로 둠) 라인 552–566 의 잔여 `let is_deduction = ...` 와 `let sign: i16 = ...` 블록도 함께 삭제됐는지 확인.

- [ ] **Step 2.3: `insert_transaction` SQL 에서 sign 제거**

`server/src/import/pipeline.rs:394-421` 의 INSERT 를 다음으로 교체:

```rust
    let txn_id: Uuid = sqlx::query_scalar!(
        r#"INSERT INTO transactions (
            owner_id, raw_id, group_id, occurred_on,
            merchant_id, actor_id, category_id, product_id, payment_method_id,
            amount, unit_price, quantity, memo
        ) VALUES (
            $1, $2, $3, $4,
            $5, $6, $7, $8, $9,
            $10, $11, $12, $13
        ) RETURNING id"#,
        owner_id,
        t.raw_id,
        t.group_id,
        t.occurred_on,
        t.merchant_id,
        t.actor_id,
        t.category_id,
        t.product_id,
        t.payment_method_id,
        t.amount,
        t.unit_price,
        t.quantity,
        t.memo,
    )
    .fetch_one(&mut *conn)
    .await?;
    Ok(txn_id)
```

- [ ] **Step 2.4: 그룹 무결성 쿼리 갱신**

`server/src/import/pipeline.rs:431-447` 의 `check_group_integrity` 쿼리 안에서 `SUM(t.amount * t.sign)` 두 곳을 모두 `-SUM(t.amount)` 로 교체. 헤더 `total_amount` 는 엑셀 원본 부호 그대로 남기고 라인은 반전 저장이므로 합산도 반전해서 비교한다.

```rust
        r#"
        SELECT
            g.group_id,
            g.header_total,
            COALESCE(-SUM(t.amount), 0) AS lines_sum
        FROM (
            SELECT group_id, total_amount AS header_total
            FROM transactions_raw
            WHERE is_group_header = true
              AND owner_id = $1
              AND import_batch_id = $2
        ) g
        LEFT JOIN transactions t ON t.group_id = g.group_id AND t.owner_id = $1
        GROUP BY g.group_id, g.header_total
        HAVING g.header_total <> COALESCE(-SUM(t.amount), 0)
        "#
```

---

## Task 3: 모든 read 엔드포인트에서 sign 제거

**Files:**
- Modify: `server/src/api/summary.rs`
- Modify: `server/src/api/transactions.rs:44,86,140`
- Modify: `server/src/api/export.rs:84-85,117-120,242-243,265-266,298-299,366`
- Modify: `server/src/api/merchant_stats.rs:82`
- Modify: `server/src/api/price.rs:92,120`

- [ ] **Step 3.1: `summary.rs` — kind 필터 + 부호 반전, sign 필드 제거**

`server/src/api/summary.rs` 전체를 다음으로 교체:

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

use crate::auth::ExtractUser;
use crate::error::AppResult;

#[derive(Debug, Serialize)]
pub struct ActorRef {
    pub actor_id: Option<Uuid>,
    pub actor_name: String,
}

#[derive(Debug, Serialize)]
pub struct ByActorEntry {
    pub actor_id: Option<Uuid>,
    pub actor_name: String,
    /// 양수 = 지출 합계, 음수 = 환불 우세. 프론트는 `Math.abs()` 로 슬라이스 크기 사용.
    pub amount: Decimal,
}

#[derive(Debug, Serialize)]
pub struct CategorySummary {
    pub category_id: Uuid,
    pub category_name: String,
    pub kind: String,
    pub by_actor: Vec<ByActorEntry>,
    pub total: Decimal,
}

#[derive(Debug, Serialize)]
pub struct SummaryResponse {
    pub year: i32,
    pub month: i32,
    pub categories: Vec<CategorySummary>,
    pub actors: Vec<ActorRef>,
}

/// GET /api/summary/:year/:month
///
/// 지출 카테고리(`kind='expense'`) 만 반환한다. 수입은 별도 엔드포인트(`/api/summary/income`).
/// amount = -SUM(t.amount) 로 양수화 (저장상 지출은 음수).
pub async fn handle_get_summary(
    State(pool): State<Arc<PgPool>>,
    ExtractUser(user): ExtractUser,
    Path((year, month)): Path<(i32, i32)>,
) -> AppResult<Json<SummaryResponse>> {
    let owner_id = user.sub;

    let rows = sqlx::query!(
        r#"
        SELECT
            c.id        AS "category_id!: Uuid",
            c.name      AS "category_name!: String",
            c.kind      AS "kind!: String",
            a.id        AS "actor_id?: Uuid",
            a.name      AS "actor_name?: String",
            (-SUM(t.amount))::numeric(15,2) AS "amount!: Decimal"
        FROM transactions t
        JOIN categories c         ON c.id = t.category_id AND c.owner_id = t.owner_id
        LEFT JOIN ledger_actors a ON a.id = t.actor_id    AND a.owner_id = t.owner_id
        WHERE t.owner_id = $1
          AND c.kind = 'expense'
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

    let mut actor_order: Vec<Option<Uuid>> = Vec::new();
    let mut actor_map: HashMap<Option<Uuid>, String> = HashMap::new();

    let mut category_order: Vec<Uuid> = Vec::new();
    let mut category_meta: HashMap<Uuid, (String, String)> = HashMap::new();
    let mut category_actors: HashMap<Uuid, Vec<ByActorEntry>> = HashMap::new();

    for row in rows {
        let actor_id = row.actor_id;
        let actor_name = row.actor_name.unwrap_or_else(|| "(미지정)".to_string());

        if !actor_map.contains_key(&actor_id) {
            actor_order.push(actor_id);
            actor_map.insert(actor_id, actor_name.clone());
        }

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
            let total: Decimal = by_actor
                .iter()
                .map(|e| e.amount)
                .fold(Decimal::ZERO, |acc, x| acc + x);
            CategorySummary {
                category_id: cid,
                category_name: name,
                kind,
                by_actor,
                total,
            }
        })
        .collect();

    let actors: Vec<ActorRef> = actor_order
        .into_iter()
        .map(|aid| ActorRef {
            actor_id: aid,
            actor_name: actor_map.remove(&aid).unwrap(),
        })
        .collect();

    Ok(Json(SummaryResponse {
        year,
        month,
        categories,
        actors,
    }))
}
```

- [ ] **Step 3.2: `transactions.rs` — sign 필드 제거**

`server/src/api/transactions.rs` 의 다음 세 자리를 수정:

라인 44 의 `pub sign: i16,` 줄 삭제.
라인 86 의 `t.sign         AS "sign!: i16",` 줄 삭제.
라인 140 의 `sign: row.sign,` 줄 삭제.

- [ ] **Step 3.3: `export.rs` — Transactions 시트의 sign 컬럼 제거 + Summary 쿼리 갱신**

`server/src/api/export.rs:84` 의 헤더 배열에서 `"sign"` 제거 → 인접 헤더 인덱스 재정렬. `kind` 컬럼을 추가한다.

라인 80~95 부근의 헤더 정의 (정확한 위치는 현재 코드 기준):

```rust
        let headers = [
            "occurred_on",
            "actor",
            "category",
            "kind",
            "merchant",
            "memo",
            "amount",
            "payment_method",
        ];
```

라인 117~125 의 셀 쓰기 부분에서 `sheet.write_number(row, 7, t.sign as f64)` 줄 삭제 + 컬럼 인덱스 재정렬. amount 컬럼은 이제 부호 있는 값을 그대로 기록한다 (지출은 음수, 수입/환불은 양수).

`TransactionExportRow` 구조체 (라인 240~245) 에서 `sign: i16,` 제거. SELECT 쿼리에서 `t.sign         AS "sign!: i16",` 제거. 응답 매핑에서 `sign: r.sign,` 제거. `kind` 를 추가:

```rust
struct TransactionExportRow {
    occurred_on: NaiveDate,
    actor: Option<String>,
    category: Option<String>,
    kind: Option<String>,  // 추가
    merchant: Option<String>,
    memo: Option<String>,
    amount: Decimal,
    payment_method: Option<String>,
}
```

해당 SELECT 에 `c.kind AS "kind?: String"` 컬럼 추가 + `LEFT JOIN categories c ON c.id = t.category_id AND c.owner_id = t.owner_id` 보강 (이미 join 되어 있으면 컬럼만 추가).

라인 366 의 Summary 시트 쿼리에서 `(SUM(t.amount * t.sign))::text` 를 `(-SUM(t.amount))::text` 로 변경 + WHERE 절에 `AND c.kind = 'expense'` 추가.

- [ ] **Step 3.4: `merchant_stats.rs` — `amount * sign` 제거**

`server/src/api/merchant_stats.rs:82` 의 `SUM(t.amount * t.sign)::numeric(15,2) AS "total!: Decimal"` 를 `(-SUM(t.amount))::numeric(15,2) AS "total!: Decimal"` 로 변경. 같은 쿼리에 `kind='expense'` 필터를 join 으로 추가:

```sql
JOIN categories c ON c.id = t.category_id AND c.owner_id = t.owner_id
WHERE ... AND c.kind = 'expense'
```

(WHERE 절 위치는 현재 쿼리 구조를 따른다.)

- [ ] **Step 3.5: `price.rs` — sign 제거**

`server/src/api/price.rs:92` 의 `t.sign         AS "sign!: i16",` 삭제.
라인 120 의 `line_amount: r.amount * Decimal::from(r.sign),` 를 `line_amount: -r.amount,` 로 변경. (가격 이력은 지출만 다루므로 양수화.) 응답 구조체에 sign 필드가 있다면 함께 제거.

---

## Task 4: sqlx 캐시 재생성 + 빌드 검증

**Files:** `server/.sqlx/` (자동 생성), `server/Cargo.lock`

- [ ] **Step 4.1: sqlx prepare 재생성**

```bash
cd server
cargo sqlx prepare --workspace -- --tests
```

기대: `query data written to .sqlx in the workspace root` 가 출력되고 종료 코드 0.

- [ ] **Step 4.2: `cargo check` 그린 확인**

```bash
cargo check -p server --tests
```

기대: 경고 외 에러 없음.

- [ ] **Step 4.3: 커밋**

```bash
git add server/
git commit -m "refactor(server): drop sign column, store cash-flow signed amount"
```

---

## Task 5: 신규 엔드포인트 — 액터별 수입 합계

**Files:**
- Create: `server/src/api/income.rs`
- Modify: `server/src/api/mod.rs:1-15,32-56`
- Test: `server/tests/test_income_split.rs`

- [ ] **Step 5.1: 실패 테스트 작성**

`server/tests/test_income_split.rs` 신규 작성:

```rust
mod helpers;
use helpers::*;
use rust_decimal::Decimal;
use uuid::Uuid;

#[sqlx::test]
async fn income_endpoint_returns_per_actor_totals(pool: sqlx::PgPool) -> anyhow::Result<()> {
    let owner_id = Uuid::new_v4();
    let actor_eonga = create_actor(&pool, owner_id, "엉아").await?;
    let _actor_baby = create_actor(&pool, owner_id, "아기").await?;
    let _actor_joint = create_actor(&pool, owner_id, "공동").await?;
    let cat_salary = create_category_with_kind(&pool, owner_id, "급여", "income").await?;

    // 급여 -3,500,000 (엑셀) → 저장 +3,500,000 (income)
    insert_transaction_with_amount(
        &pool,
        owner_id,
        actor_eonga,
        cat_salary,
        "2026-02-25",
        Decimal::from(3_500_000),
    )
    .await?;

    let token = make_token(owner_id);
    let app = build_app(pool.clone()).await;
    let response = http_get(&app, "/api/summary/income/2026/2", &token).await;
    assert_eq!(response.status, 200);

    let body: serde_json::Value = serde_json::from_slice(&response.body)?;
    assert_eq!(body["month"], "2026-02");
    let by_actor = body["by_actor"].as_array().unwrap();
    let eonga = by_actor.iter().find(|e| e["actor_name"] == "엉아").unwrap();
    assert_eq!(eonga["total"], "3500000.00");
    let baby = by_actor.iter().find(|e| e["actor_name"] == "아기").unwrap();
    assert_eq!(baby["total"], "0.00");
    assert_eq!(body["total"], "3500000.00");

    Ok(())
}
```

(`helpers` 모듈에 `create_category_with_kind` 와 `insert_transaction_with_amount` 가 없다면 본 태스크에서 추가한다 — 기존 `tests/helpers.rs` 또는 `mod helpers;` 위치 확인 후 가장 가까운 헬퍼 양식을 따른다.)

- [ ] **Step 5.2: 테스트 실행하여 실패 확인**

```bash
cd server && cargo test -p server --test test_income_split -- --nocapture 2>&1 | tail -20
```

기대: 컴파일 실패(엔드포인트 미구현) 또는 404.

- [ ] **Step 5.3: `income.rs` 핸들러 구현**

`server/src/api/income.rs` 신규 작성:

```rust
use axum::{
    extract::{Path, State},
    Json,
};
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

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
}

/// GET /api/summary/income/:year/:month
///
/// 해당 월의 `kind='income'` 카테고리 트랜잭션을 액터별로 합산한다.
/// 저장 규약상 수입은 양수이므로 그대로 SUM(amount).
/// 등록된 모든 액터를 결과에 포함하되 거래 없는 액터는 total=0 으로 채운다.
pub async fn handle_get_income(
    State(pool): State<Arc<PgPool>>,
    ExtractUser(user): ExtractUser,
    Path((year, month)): Path<(i32, i32)>,
) -> AppResult<Json<IncomeResponse>> {
    let owner_id = user.sub;

    let rows = sqlx::query!(
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
                0
            )::numeric(15,2) AS "total!: Decimal"
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

    let by_actor: Vec<IncomeByActor> = rows
        .into_iter()
        .map(|r| IncomeByActor {
            actor_id: Some(r.actor_id),
            actor_name: r.actor_name,
            total: r.total,
        })
        .collect();

    let total: Decimal = by_actor.iter().map(|e| e.total).sum();

    Ok(Json(IncomeResponse {
        month: format!("{:04}-{:02}", year, month),
        by_actor,
        total,
    }))
}
```

- [ ] **Step 5.4: 라우터 등록**

`server/src/api/mod.rs` 상단 `pub mod` 목록에 `pub mod income;` 추가, 그리고 `protected` Router 에 다음 줄 추가 (`/api/summary/:year/:month` 다음 줄):

```rust
        .route("/api/summary/income/:year/:month", get(income::handle_get_income))
```

- [ ] **Step 5.5: sqlx prepare 재생성 + 테스트 실행**

```bash
cd server && cargo sqlx prepare --workspace -- --tests
cargo test -p server --test test_income_split 2>&1 | tail -10
```

기대: 1 passed.

- [ ] **Step 5.6: 커밋**

```bash
git add server/
git commit -m "feat(server): GET /api/summary/income/:year/:month — per-actor income totals"
```

---

## Task 6: 신규 엔드포인트 — `PATCH /api/categories/:id/kind`

**Files:**
- Modify: `server/src/api/categories.rs`
- Modify: `server/src/api/mod.rs`
- Test: `server/tests/test_category_kind.rs`

- [ ] **Step 6.1: 실패 테스트 작성**

`server/tests/test_category_kind.rs` 신규 작성:

```rust
mod helpers;
use helpers::*;
use uuid::Uuid;

#[sqlx::test]
async fn patch_category_kind_flips_value(pool: sqlx::PgPool) -> anyhow::Result<()> {
    let owner_id = Uuid::new_v4();
    let cat = create_category_with_kind(&pool, owner_id, "급여", "expense").await?;

    let token = make_token(owner_id);
    let app = build_app(pool.clone()).await;
    let response = http_patch_json(
        &app,
        &format!("/api/categories/{}/kind", cat),
        &token,
        serde_json::json!({"kind": "income"}),
    )
    .await;
    assert_eq!(response.status, 200);

    let body: serde_json::Value = serde_json::from_slice(&response.body)?;
    assert_eq!(body["kind"], "income");

    let row = sqlx::query!("SELECT kind FROM categories WHERE id = $1", cat)
        .fetch_one(&pool)
        .await?;
    assert_eq!(row.kind, "income");

    Ok(())
}

#[sqlx::test]
async fn patch_category_kind_rejects_invalid_value(pool: sqlx::PgPool) -> anyhow::Result<()> {
    let owner_id = Uuid::new_v4();
    let cat = create_category_with_kind(&pool, owner_id, "급여", "expense").await?;
    let token = make_token(owner_id);
    let app = build_app(pool.clone()).await;
    let response = http_patch_json(
        &app,
        &format!("/api/categories/{}/kind", cat),
        &token,
        serde_json::json!({"kind": "rubbish"}),
    )
    .await;
    assert!(response.status == 400 || response.status == 422);
    Ok(())
}

#[sqlx::test]
async fn patch_category_kind_protects_deduction(pool: sqlx::PgPool) -> anyhow::Result<()> {
    let owner_id = Uuid::new_v4();
    let cat = create_category_with_kind(&pool, owner_id, "차감", "expense").await?;
    let token = make_token(owner_id);
    let app = build_app(pool.clone()).await;
    let response = http_patch_json(
        &app,
        &format!("/api/categories/{}/kind", cat),
        &token,
        serde_json::json!({"kind": "income"}),
    )
    .await;
    assert_eq!(response.status, 409);
    Ok(())
}
```

(`http_patch_json` 헬퍼가 없다면 기존 `http_get` / `http_post_json` 을 모방해 추가한다.)

- [ ] **Step 6.2: 테스트 실행하여 실패 확인**

```bash
cd server && cargo test -p server --test test_category_kind 2>&1 | tail -20
```

기대: 컴파일 실패 또는 404 / 405.

- [ ] **Step 6.3: 핸들러 구현**

`server/src/api/categories.rs` 끝에 다음 핸들러 추가:

```rust
use axum::extract::Path;
use axum::http::StatusCode;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct PatchCategoryKindBody {
    pub kind: String,
}

#[derive(Debug, Serialize)]
pub struct PatchCategoryKindResponse {
    pub id: Uuid,
    pub kind: String,
}

/// PATCH /api/categories/:id/kind
pub async fn handle_patch_category_kind(
    State(pool): State<Arc<PgPool>>,
    ExtractUser(user): ExtractUser,
    Path(category_id): Path<Uuid>,
    Json(body): Json<PatchCategoryKindBody>,
) -> Result<Json<PatchCategoryKindResponse>, (StatusCode, String)> {
    let owner_id = user.sub;

    if body.kind != "income" && body.kind != "expense" {
        return Err((StatusCode::BAD_REQUEST, "kind must be 'income' or 'expense'".into()));
    }

    // 차감 보호: 시스템 카테고리는 토글 금지.
    let row = sqlx::query!(
        r#"SELECT name AS "name!: String" FROM categories WHERE id = $1 AND owner_id = $2"#,
        category_id,
        owner_id
    )
    .fetch_optional(&*pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let Some(found) = row else {
        return Err((StatusCode::NOT_FOUND, "category not found".into()));
    };
    if found.name == "차감" {
        return Err((StatusCode::CONFLICT, "차감 is a protected category".into()));
    }

    sqlx::query!(
        r#"UPDATE categories SET kind = $1 WHERE id = $2 AND owner_id = $3"#,
        body.kind,
        category_id,
        owner_id
    )
    .execute(&*pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(PatchCategoryKindResponse {
        id: category_id,
        kind: body.kind,
    }))
}
```

- [ ] **Step 6.4: 라우터 등록**

`server/src/api/mod.rs` 의 `protected` 빌더에 다음 줄 추가:

```rust
        .route(
            "/api/categories/:id/kind",
            axum::routing::patch(categories::handle_patch_category_kind),
        )
```

(상단 `use axum::routing::{...};` 에 `patch` 가 없으면 추가.)

- [ ] **Step 6.5: sqlx prepare + 테스트**

```bash
cd server && cargo sqlx prepare --workspace -- --tests
cargo test -p server --test test_category_kind 2>&1 | tail -10
```

기대: 3 passed.

- [ ] **Step 6.6: 커밋**

```bash
git add server/
git commit -m "feat(server): PATCH /api/categories/:id/kind for income/expense toggle"
```

---

## Task 7: 기존 백엔드 테스트 갱신

**Files:**
- Modify: `server/tests/test_import_integration.rs`
- Modify: `server/tests/test_m2_step_a.rs`
- Modify: `server/tests/test_m2_step_b.rs`
- Modify: `server/tests/test_m4_export.rs`
- Modify: `server/tests/test_m3.rs`
- Modify: `server/tests/test_normalize.rs` (해당하면)

- [ ] **Step 7.1: 정산 / summary 기댓값 새 부호로 재계산**

`test_m2_step_a.rs`, `test_m2_step_b.rs` 의 정산/Summary 단언문에서:
- `recognized_expense` / `deducted_amount` 기댓값은 양수로 변하지 않으므로 대부분 그대로.
- summary `by_actor[].sign` 필드 단언이 있으면 모두 삭제 (응답 DTO 에서 제거됨).
- summary `amount` 단언은 이전과 동일한 양수값(= -SUM(stored)) 이므로 대부분 그대로.

각 테스트 파일을 한 번씩 열고 `sign` 키워드를 검색해 `assert*` 라인을 정리.

- [ ] **Step 7.2: import 무결성 테스트의 sign 의존 제거**

`test_import_integration.rs` 에서 group integrity 검사 로직을 사용 중이면, 이제 `transactions.amount` 가 부호 반전 저장이라 헤더 합과 비교가 자동으로 작동한다. 직접 `amount * sign` 을 계산하는 부분이 있으면 `-amount` 로 교체.

- [ ] **Step 7.3: export 헤더 단언 갱신**

`test_m4_export.rs` 가 시트 헤더에 `sign` 컬럼을 단언하면 이를 제거하고 `kind` 단언을 추가.

- [ ] **Step 7.4: m3 (price-history) 단언 갱신**

`test_m3.rs` 의 price-history 응답에서 `sign` 또는 `line_amount` 부호 가정 부분을 새 규약으로 갱신 (line_amount 이제 양수 = 지출 크기).

- [ ] **Step 7.5: 전체 테스트 실행**

```bash
cd server && cargo test -p server 2>&1 | tail -20
```

기대: 모든 기존 + 신규 테스트 green. 실패 케이스는 단언 갱신으로 해결.

- [ ] **Step 7.6: 커밋**

```bash
git add server/tests/
git commit -m "test(server): update tests for signed-amount cash-flow convention"
```

---

## Task 8: DB wipe + 재임포트 + 수동 검증

**Files:** 없음 (운영 작업)

- [ ] **Step 8.1: 로컬 DB drop & migrate**

```bash
cd server
psql "$DATABASE_URL" -c "DROP SCHEMA public CASCADE; CREATE SCHEMA public;"
sqlx migrate run
```

- [ ] **Step 8.2: 서버 기동**

```bash
cargo run -p server
```

(별 터미널에서 진행)

- [ ] **Step 8.3: `2026년 02월.xlsx` 재임포트**

웹 `/import` UI 또는 curl 로 임포트. 응답의 `transactions_inserted` 가 기존 M3 메모(`memo-less row count 64`) 와 같은 차수에서 일치하는지 확인.

- [ ] **Step 8.4: 데이터 검수 쿼리**

```bash
psql "$DATABASE_URL" <<'SQL'
-- 부호 분포: 음수 (지출) 압도적 + 양수 (환불/수입) 일부
SELECT
  SIGN(amount) AS dir,
  COUNT(*),
  SUM(amount)::numeric(15,2) AS total
FROM transactions
GROUP BY SIGN(amount)
ORDER BY dir;

-- 수입 카테고리 (kind='income') 총합
SELECT c.name, SUM(t.amount)::numeric(15,2) AS total
FROM transactions t
JOIN categories c ON c.id = t.category_id
WHERE c.kind = 'income'
GROUP BY c.name
ORDER BY total DESC;

-- 정산
SELECT * FROM v_monthly_settlement WHERE month = '2026-02-01';
SQL
```

기대:
- 음수 행이 압도적, 양수 행은 환불 + 수입.
- `급여`/`이자` 등 수입 카테고리가 `kind='income'` 으로 식별되려면 사용자가 `/aliases` 에서 토글한 후가 정확. 임포트 직후엔 모두 `expense` 로 들어와 있을 것.
- `v_monthly_settlement` 의 `recognized_expense` 가 양수, `deducted_amount` 가 양수, `settlement_input = recognized − deducted` 성립.

- [ ] **Step 8.5: 수입 카테고리 토글 (수동)**

운영 측 — DB 직접 또는 추후 추가될 UI 로 `급여`, `이자` 등의 카테고리 `kind` 를 `income` 으로 갱신해 보고 `/api/summary/income/2026/2` 결과가 합리적인지 확인.

```sql
UPDATE categories SET kind = 'income' WHERE name IN ('급여') AND owner_id = '<your-uuid>';
```

(이자 등 세부 카테고리는 데이터에 따라 조정.)

---

## Task 9: 프론트엔드 — 스키마 + 헬퍼에서 sign 제거

**Files:**
- Modify: `web/lib/schemas.ts:13-66,123-130`
- Modify: `web/lib/donut-data.ts:33-37,55-70`
- Modify: `web/lib/utils.ts:20-29`
- Modify: `web/components/transactions-table.tsx:168-184`

- [ ] **Step 9.1: 실패 테스트 작성/갱신**

`web/__tests__/donut-data.test.ts` 의 fixture 헬퍼 시그니처에서 `sign` 을 제거하고 `amount` 가 이미 부호 있는 값임을 가정하도록 갱신:

```ts
function makeSummary(
  categories: Array<{ name: string; cells: Array<{ actor: string; amount: string }> }>,
  ...
): SummaryResponse {
  return {
    year: 2026,
    month: 2,
    categories: categories.map((c) => ({
      ...,
      by_actor: c.cells.map((cell) => ({
        actor_id: cell.actor,
        actor_name: ...,
        amount: cell.amount,  // sign 키 제거
      })),
    })),
    actors: ...,
  };
}
```

기존 `it("respects sign=-1 (refund / negative line) when summing")` 테스트는 이름과 fixture 를 바꿔 환불을 음수 amount 로 직접 표현:

```ts
it("환불(음수 amount)이 같은 actor 의 합계를 깎는다", () => {
  const data = makeSummary([
    { name: "외식", cells: [{ actor: ACTOR_A, amount: "1000" }] },
    { name: "환불", cells: [{ actor: ACTOR_A, amount: "-300" }] },
  ], [...]);
  const result = buildActorSlices(data, ACTOR_A);
  expect(result.total).toBe(700);
});
```

- [ ] **Step 9.2: 테스트 실행하여 실패 확인**

```bash
cd web && npm test -- --run donut-data 2>&1 | tail -20
```

기대: 컴파일 실패(타입) 또는 단언 실패.

- [ ] **Step 9.3: `schemas.ts` 에서 sign 제거**

`web/lib/schemas.ts:13-33` 의 `TransactionItem` 타입에서 `sign: number;` 줄 제거. 라인 53 의 `sign: z.number().int(),` 줄 제거. 라인 123-128 의 `ByActorEntrySchema` 에서 `sign: z.number().int(),` 줄 제거.

- [ ] **Step 9.4: `donut-data.ts` 에서 signedNumber 제거**

`web/lib/donut-data.ts:33-37` 의 `signedNumber` 함수 삭제. 라인 55-70 의 `buildActorSlices` 에서 `const v = signedNumber(cell.amount, cell.sign);` 를 `const v = parseFloat(cell.amount); if (Number.isNaN(v)) continue;` 로 교체. 나머지 로직(top N + 차감 핀 + total)은 그대로.

- [ ] **Step 9.5: `utils.ts` formatAmount 갱신**

`web/lib/utils.ts:20-29` 의 `formatAmount` 를 단일 인자로 단순화:

```ts
/**
 * Decimal string 을 천단위 콤마 + 음수면 - 접두사로 표시.
 * sign 인자는 폐기됨 — amount 자체에 부호가 들어 있다.
 */
export function formatAmount(amount: string | null | undefined): string {
  if (amount == null) return "";
  const v = parseFloat(amount);
  if (Number.isNaN(v)) return amount;
  const formatted = Math.abs(v).toLocaleString();
  return v < 0 ? `-${formatted}` : formatted;
}
```

호출부(`web/components/transactions-table.tsx:168-184`) 갱신:

```tsx
{
  accessorFn: (row) => row.item.amount,
  cell: ({ getValue }) => {
    const amount = getValue<string>();
    const isNegative = parseFloat(amount) < 0;
    return (
      <span className={cn(
        "font-mono text-right",
        isNegative ? "text-blue-600" : "text-foreground",
      )}>
        {formatAmount(amount)}
      </span>
    );
  },
  ...
}
```

`utils.test.ts` 의 인자 변경에 맞춰 단언 갱신.

- [ ] **Step 9.6: 테스트 실행**

```bash
cd web && npm test 2>&1 | tail -10
```

기대: 단언이 새 시그니처로 통과.

- [ ] **Step 9.7: 커밋**

```bash
git add web/
git commit -m "refactor(web): drop sign field; amount is signed cash-flow"
```

---

## Task 10: 프론트엔드 — IncomeStrip 컴포넌트

**Files:**
- Create: `web/components/income-strip.tsx`
- Modify: `web/lib/schemas.ts`
- Modify: `web/app/(app)/page.tsx`
- Test: `web/__tests__/income-strip.test.tsx`

- [ ] **Step 10.1: 스키마 추가**

`web/lib/schemas.ts` 끝에 추가:

```ts
export const IncomeByActorSchema = z.object({
  actor_id: z.string().uuid().nullable(),
  actor_name: z.string(),
  total: DecimalSchema,
});

export const IncomeResponseSchema = z.object({
  month: z.string(),
  by_actor: z.array(IncomeByActorSchema),
  total: DecimalSchema,
});

export type IncomeResponse = z.infer<typeof IncomeResponseSchema>;
```

- [ ] **Step 10.2: 실패 테스트 작성**

`web/__tests__/income-strip.test.tsx` 신규 작성:

```tsx
import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { IncomeStrip } from "@/components/income-strip";
import type { IncomeResponse } from "@/lib/schemas";

const data: IncomeResponse = {
  month: "2026-02",
  by_actor: [
    { actor_id: "a1", actor_name: "공동", total: "0" },
    { actor_id: "a2", actor_name: "엉아", total: "3500000" },
    { actor_id: "a3", actor_name: "아기", total: "0" },
  ],
  total: "3500000",
};

describe("IncomeStrip", () => {
  it("액터별 수입을 표시한다", () => {
    render(<IncomeStrip data={data} />);
    expect(screen.getByText("공동")).toBeInTheDocument();
    expect(screen.getByText("엉아")).toBeInTheDocument();
    expect(screen.getByText("아기")).toBeInTheDocument();
    expect(screen.getByText(/3,500,000/)).toBeInTheDocument();
  });

  it("거래 없는 액터도 ₩0 으로 보여준다", () => {
    render(<IncomeStrip data={data} />);
    const zeros = screen.getAllByText(/₩\s*0\b/);
    expect(zeros.length).toBeGreaterThanOrEqual(2);
  });

  it("data=null 이면 placeholder 만 보여주거나 렌더 생략", () => {
    const { container } = render(<IncomeStrip data={null} />);
    expect(container.textContent ?? "").not.toContain("undefined");
  });
});
```

- [ ] **Step 10.3: 테스트 실행하여 실패 확인**

```bash
cd web && npm test -- --run income-strip 2>&1 | tail -20
```

기대: 컴파일 실패 (`IncomeStrip` 미존재).

- [ ] **Step 10.4: `IncomeStrip` 구현**

`web/components/income-strip.tsx` 신규:

```tsx
import type { IncomeResponse } from "@/lib/schemas";
import { Card, CardContent } from "@/components/ui/card";

interface Props {
  data: IncomeResponse | null;
}

function formatKRW(amount: string): string {
  const v = parseFloat(amount);
  if (Number.isNaN(v)) return "₩0";
  return `₩${Math.round(v).toLocaleString()}`;
}

export function IncomeStrip({ data }: Props) {
  if (!data) return null;

  return (
    <Card>
      <CardContent className="py-3">
        <div className="flex items-center gap-6">
          <span className="text-sm font-medium text-muted-foreground">월 수입</span>
          <div className="flex items-center gap-4 flex-wrap">
            {data.by_actor.map((row) => (
              <div key={row.actor_id ?? row.actor_name} className="flex items-center gap-1.5">
                <span className="text-sm text-muted-foreground">{row.actor_name}</span>
                <span className="text-sm font-mono">{formatKRW(row.total)}</span>
              </div>
            ))}
          </div>
          <div className="ml-auto text-sm font-mono font-semibold">
            합계 {formatKRW(data.total)}
          </div>
        </div>
      </CardContent>
    </Card>
  );
}
```

- [ ] **Step 10.5: 대시보드에 통합**

`web/app/(app)/page.tsx` 수정:

`import` 블록에 추가:

```tsx
import { IncomeStrip } from "@/components/income-strip";
import { IncomeResponseSchema, IncomeResponse } from "@/lib/schemas";
```

`fetchSummary` 다음에 추가:

```tsx
async function fetchIncome(year: number, month: number): Promise<IncomeResponse | null> {
  try {
    return await apiFetch<IncomeResponse>(`/api/summary/income/${year}/${month}`, {
      schema: IncomeResponseSchema,
    });
  } catch {
    return null;
  }
}

async function IncomeSection({ year, month }: { year: number; month: number }) {
  const data = await fetchIncome(year, month);
  return <IncomeStrip data={data} />;
}
```

`SettlementSection` 과 `DashboardDonutsSection` 사이에 새 `<Suspense>` 삽입:

```tsx
      <Suspense
        key={`income-${sectionKey}`}
        fallback={<StripSkeleton />}
      >
        <IncomeSection year={year} month={month} />
      </Suspense>
```

- [ ] **Step 10.6: 테스트 실행**

```bash
cd web && npm test -- --run income-strip 2>&1 | tail -10
cd web && npm test -- --run dashboard 2>&1 | tail -10
```

기대: income-strip 3 passed. dashboard 는 IncomeSection mock 추가 필요할 수 있음 → fetch mock 헬퍼에 `/api/summary/income/...` 응답을 추가하고 단언은 IncomeStrip 존재 여부를 확인하는 한 줄만 추가.

- [ ] **Step 10.7: 커밋**

```bash
git add web/
git commit -m "feat(web): per-actor income strip on dashboard"
```

---

## Task 11: `/aliases` 카테고리 탭 — kind 토글 스위치

**Files:**
- Modify: `web/app/(app)/aliases/page.tsx` 또는 `web/components/aliases/categories-tab.tsx` (실제 위치 확인)
- Test: `web/__tests__/aliases.test.tsx`

(현 구조에 따라 위치가 달라지므로 작업 시작 시 `grep -rn "categories" web/app/\(app\)/aliases/ web/components/aliases/ 2>/dev/null` 로 정확한 파일을 확인.)

- [ ] **Step 11.1: 실패 테스트 추가**

`web/__tests__/aliases.test.tsx` 에 케이스 추가:

```tsx
it("category kind 토글이 PATCH 호출을 발생시키고 UI 가 갱신된다", async () => {
  const fetchMock = vi.fn().mockImplementation(async (url: string, init?: RequestInit) => {
    if (init?.method === "PATCH" && url.includes("/api/categories/") && url.endsWith("/kind")) {
      return new Response(JSON.stringify({ id: "cat-1", kind: "income" }), { status: 200 });
    }
    // 기존 GET 모킹은 그대로
    ...
  });
  global.fetch = fetchMock;

  render(<AliasesPage ... />);
  const toggle = await screen.findByRole("switch", { name: /급여/ });
  await userEvent.click(toggle);

  expect(fetchMock).toHaveBeenCalledWith(
    expect.stringMatching(/\/api\/categories\/cat-1\/kind$/),
    expect.objectContaining({ method: "PATCH" }),
  );
});

it("차감 카테고리는 토글이 비활성화된다", async () => {
  ...
  const toggle = await screen.findByRole("switch", { name: /차감/ });
  expect(toggle).toBeDisabled();
});
```

- [ ] **Step 11.2: 테스트 실행하여 실패 확인**

```bash
cd web && npm test -- --run aliases 2>&1 | tail -20
```

- [ ] **Step 11.3: 카테고리 탭에 `<Switch>` 통합**

각 카테고리 행 렌더링부에 (실제 컴포넌트 파일 기준):

```tsx
import { Switch } from "@/components/ui/switch";

// 행 안에서:
<div className="flex items-center gap-2">
  <Switch
    aria-label={category.name}
    checked={category.kind === "income"}
    disabled={category.name === "차감"}
    onCheckedChange={async (checked) => {
      const next = checked ? "income" : "expense";
      const res = await fetch(`/api/categories/${category.id}/kind`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        credentials: "include",
        body: JSON.stringify({ kind: next }),
      });
      if (!res.ok) {
        toast.error("카테고리 종류 변경 실패");
        return;
      }
      // SWR/React Query 사용 중이면 mutate; 없으면 router.refresh() 호출.
      router.refresh();
    }}
  />
  <span className="text-xs text-muted-foreground">
    {category.kind === "income" ? "수입" : "지출"}
  </span>
</div>
```

(`Switch` 가 shadcn 에 없으면 `npx shadcn-ui@latest add switch` 로 추가.)

- [ ] **Step 11.4: 테스트 + lint**

```bash
cd web && npm test -- --run aliases 2>&1 | tail -10
cd web && npm run lint 2>&1 | tail -10
```

- [ ] **Step 11.5: 커밋**

```bash
git add web/
git commit -m "feat(web): inline kind toggle on /aliases categories tab"
```

---

## Task 12: 종합 회귀 테스트

**Files:** 없음 (테스트 실행)

- [ ] **Step 12.1: 백엔드 전체 테스트**

```bash
cd server && cargo test -p server 2>&1 | tail -20
```

기대: 신규 + 기존 모두 green.

- [ ] **Step 12.2: 프론트 전체 테스트**

```bash
cd web && npm test 2>&1 | tail -20
```

기대: 모두 green. (기존 파일 테스트의 sign 제거 단언이 누락되어 실패하면 본 태스크에서 수정.)

- [ ] **Step 12.3: 빌드 확인**

```bash
cd web && npm run build 2>&1 | tail -10
cd server && cargo build --release -p server 2>&1 | tail -10
```

- [ ] **Step 12.4: 시각 검증 (브라우저)**

```bash
docker compose up -d postgres
cd server && cargo run -p server &
cd web && npm run dev
```

- 브라우저에서 `/` → 헤더 / 정산 카드 / 수입 스트립 / 도넛 그리드 순으로 보이는지.
- `/aliases` → 카테고리 탭에서 스위치 클릭 후 UI 즉시 반영, 새로고침 후에도 유지.
- `/transactions` → 음수 amount 행이 파란색으로, 양수가 기본 색으로.

문제 있으면 본 태스크에서 수정 후 재커밋.

---

## Task 13: 문서 갱신 + 마무리

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 13.1: Core Domain Rules 갱신**

`CLAUDE.md` 의 "Transaction data" 절에서 다음 줄을 교체:

기존: `- Negative expenses are stored with \`sign = -1\` (no separate table).`
신규: `- \`transactions.amount\` is a signed cash-flow value (cash-in positive, cash-out negative). Income vs expense classification lives in \`categories.kind\` ('income' | 'expense'). Refunds are negative-amount rows in expense categories — not income.`

- [ ] **Step 13.2: Cumulative Context 추가**

`CLAUDE.md` 의 Cumulative Context 섹션에 한 줄 추가:

```
- 2026-05-07: Income/expense split + signed-amount convention — `transactions.sign` dropped; `amount` is now signed cash-flow (in+/out−). `categories.kind` is the income/expense classifier. Backend: `001_init.sql` rewrite, all queries refactored, new `GET /api/summary/income/:year/:month` and `PATCH /api/categories/:id/kind`. Frontend: dashboard `IncomeStrip`, `/aliases` kind toggle. Spec/plan: `docs/superpowers/{specs,plans}/2026-05-07-income-expense-sign-*`.
```

- [ ] **Step 13.3: 커밋**

```bash
git add CLAUDE.md
git commit -m "docs: log income/expense sign convention rewrite"
```

- [ ] **Step 13.4: 최종 빌드 + 테스트 그린 재확인**

```bash
cd server && cargo test -p server 2>&1 | tail -5
cd web && npm test 2>&1 | tail -5
```

---

## 완료 조건

- 모든 백엔드/프론트엔드 테스트 green.
- `transactions.sign` 컬럼이 DB / 코드 / 응답 어디에도 남아 있지 않음 (`grep -rn "sign" server/src web/lib web/components | grep -v "// \|signed\|design\|signing"` 으로 확인).
- 대시보드에 수입 스트립이 보이고 0 인 액터도 ₩0 으로 표시.
- `/aliases` 카테고리 탭의 토글이 PATCH 호출 후 UI 즉시 반영.
- `2026년 02월.xlsx` 재임포트 후 `v_monthly_settlement` 의 입금액이 음수가 아니고 합리적 값.
- `CLAUDE.md` 의 도메인 규칙과 누적 컨텍스트가 새 규약을 반영.
