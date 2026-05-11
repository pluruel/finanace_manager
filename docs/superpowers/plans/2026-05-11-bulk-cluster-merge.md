# Bulk Cluster Merge Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `/aliases` 페이지에 "클러스터" 탭을 추가해, 사용자가 pg_trgm 기반으로 묶인 유사 product/merchant 후보를 한 번에 병합할 수 있게 한다.

**Architecture:** Postgres `pg_trgm` extension + GIN trgm 인덱스로 같은 owner 내 유사 페어를 빠르게 추출 → Rust 측 union-find 로 컴포넌트 묶음 → 사용자가 카드 단위로 대표/흡수 선택 후 단일 트랜잭션 (UPDATE transactions + DELETE aliases + DELETE absorbed entities) 으로 병합. import-time 추천 / 배지 / 토스트 없음, `/aliases` 클러스터 탭 단일 진입.

**Tech Stack:** Rust (axum, SeaORM 1.x, FromQueryResult), PostgreSQL 17 + pg_trgm, Next.js 15 App Router, shadcn/ui, vitest.

**Spec:** `docs/superpowers/specs/2026-05-11-bulk-cluster-merge-design.md`

---

## File Structure

**Backend (`server/`)**
- Modify: `migration/src/m20260510_000001_init.rs` — `CREATE EXTENSION pg_trgm` + GIN trgm 인덱스 두 개 in-place 추가
- Create: `src/api/clusters.rs` — `GET /api/clusters`, `POST /api/clusters/merge` 핸들러 + 클러스터 계산 / 병합 로직
- Modify: `src/api/mod.rs` — `pub mod clusters;` + 두 라우트 등록
- Create: `tests/test_clusters.rs` — 9개 통합 테스트

**Frontend (`web/`)**
- Modify: `lib/schemas.ts` — `ClusterMemberSchema`, `ClusterSchema`, `ClustersResponseSchema`, `MergeRequest/Response`
- Create: `lib/cluster-data.ts` — `pickDefaultCanonical`, `sortMembersForDisplay`, `formatLatestSeen` 순수 함수
- Create: `components/cluster-card.tsx` — 한 클러스터 = 한 카드. 라디오/체크박스/병합 버튼. 클라이언트 컴포넌트.
- Create: `components/cluster-tab.tsx` — Products/Merchants 서브토글 + 임계치 입력 + "다시 계산" 버튼 + 카드 그리드. 클라이언트 컴포넌트.
- Modify: `app/(app)/aliases/page.tsx` — 5번째 탭 "클러스터" 추가
- Create: `__tests__/clusters.test.tsx` — vitest, 6개 케이스 (헬퍼 단위 + 컴포넌트 통합)

각 파일은 단일 책임:
- `clusters.rs` 안에서 cluster compute / merge 로직이 한 모듈에 묶이지만, 내부 함수 단위로 split 가능 (compute_clusters / merge_cluster). 별도 파일 분리는 YAGNI.
- 프론트는 헬퍼(`lib/`) ↔ presentational 컴포넌트(`cluster-card.tsx`) ↔ 데이터 fetch 가진 컨테이너(`cluster-tab.tsx`) 로 layered.

---

## Task 1: 마이그레이션 — pg_trgm extension + GIN trgm 인덱스

**Files:**
- Modify: `server/migration/src/m20260510_000001_init.rs:124-128` (pgcrypto 옆), 그리고 그 직후
- Test: `server/tests/test_clusters.rs` (Task 2 에서 추가, 본 task 는 컴파일/마이그레이션 동작만 확인)

- [ ] **Step 1: pg_trgm extension 추가**

`server/migration/src/m20260510_000001_init.rs` 의 `up()` 함수 안에서 `pgcrypto` extension 다음 줄 (현재 라인 127 근처) 에 추가:

```rust
        // pgcrypto for gen_random_uuid()
        conn.execute_unprepared("CREATE EXTENSION IF NOT EXISTS pgcrypto")
            .await?;

        // pg_trgm for trigram similarity (used by /api/clusters)
        conn.execute_unprepared("CREATE EXTENSION IF NOT EXISTS pg_trgm")
            .await?;
```

- [ ] **Step 2: GIN trgm 인덱스 추가**

같은 파일에서 `transactions` 인덱스 생성 블록 (현재 540~547 라인 근처, `for (name, cols) in [...]` 다음, `v_monthly_settlement` 뷰 생성 직전) 에 추가:

```rust
        // GIN trgm indexes for /api/clusters similarity search
        conn.execute_unprepared(
            r#"
            CREATE INDEX IF NOT EXISTS idx_products_name_trgm
              ON products USING gin (name gin_trgm_ops);
            CREATE INDEX IF NOT EXISTS idx_merchants_name_trgm
              ON merchants USING gin (name gin_trgm_ops);
            "#,
        )
        .await?;
```

- [ ] **Step 3: dev DB 갈아엎고 마이그레이션 검증**

Run:
```bash
DATABASE_URL=postgres://app:app@localhost:5432/finance \
  cargo run -p migration -- fresh
```
Expected: 에러 없이 종료. 마지막 메시지 "Migration 'm20260510_000001_init' has been applied".

- [ ] **Step 4: 인덱스 / extension 존재 확인**

Run:
```bash
docker compose exec postgres psql -U app -d finance -c \
  "SELECT extname FROM pg_extension WHERE extname='pg_trgm';"
docker compose exec postgres psql -U app -d finance -c \
  "SELECT indexname FROM pg_indexes WHERE indexname IN ('idx_products_name_trgm','idx_merchants_name_trgm');"
```
Expected: `pg_trgm` 한 줄, 두 인덱스 이름 두 줄.

- [ ] **Step 5: 골든 데이터 재import + 기존 테스트 회귀 없음 확인**

Run:
```bash
DATABASE_URL=postgres://app:app@localhost:5432/postgres cargo test -p finance-manager
```
Expected: 모든 기존 테스트 통과 (87/87 또는 그 이상). 새 테스트는 아직 없음.

- [ ] **Step 6: Commit**

```bash
git add server/migration/src/m20260510_000001_init.rs
git commit -m "feat(db): add pg_trgm extension + GIN trgm indexes on products/merchants names"
```

---

## Task 2: Backend — 클러스터 계산 핵심 함수 (TDD)

**Files:**
- Create: `server/src/api/clusters.rs`
- Test: `server/tests/test_clusters.rs`

이 task 는 endpoint 없이 **순수 계산 함수**(`compute_clusters`) 만 작성. union-find 묶음 + 멤버 메타 채우기. endpoint wiring 은 Task 3 에서.

- [ ] **Step 1: 테스트 파일 생성 (failing)**

Create `server/tests/test_clusters.rs`:

```rust
//! /api/clusters integration tests
mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware, routing, Router,
};
use finance_manager::auth::AuthUser;
use finance_manager::entity::{import_batches, prelude::ImportBatches};
use finance_manager::import::pipeline::run_pipeline;
use finance_manager::import::xlsx::{extract_sheet_name, extract_year_month, parse_xlsx};
use sea_orm::{ActiveValue::Set, DatabaseConnection, EntityTrait, TransactionTrait};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

fn load_golden_bytes() -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/2026년_02월.xlsx");
    std::fs::read(path).expect("Failed to read golden xlsx fixture")
}

async fn do_import(t: &common::TestDb, owner_id: Uuid) {
    let bytes = load_golden_bytes();
    let filename = "2026년 02월.xlsx";
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let hash_vec = hasher.finalize().to_vec();
    let (year, month) = extract_year_month(filename).unwrap();
    let sheet_name = extract_sheet_name(filename).unwrap();
    let raw_rows = parse_xlsx(&bytes, &sheet_name).unwrap();
    let row_count = raw_rows.len() as i32;

    let txn = t.db.begin().await.unwrap();
    let batch_id = ImportBatches::insert(import_batches::ActiveModel {
        owner_id: Set(owner_id),
        file_name: Set(filename.to_string()),
        file_hash: Set(hash_vec),
        year: Set(year),
        month: Set(month),
        row_count: Set(row_count),
        ..Default::default()
    })
    .exec(&txn)
    .await
    .unwrap()
    .last_insert_id;
    run_pipeline(&txn, owner_id, batch_id, raw_rows).await.unwrap();
    txn.commit().await.unwrap();
}

fn build_test_router(db: Arc<DatabaseConnection>, owner_id: Uuid) -> Router {
    let user = AuthUser {
        sub: owner_id,
        email: "test@example.com".to_string(),
        groups: vec![],
    };
    Router::new()
        .route(
            "/api/clusters",
            routing::get(finance_manager::api::clusters::handle_get_clusters),
        )
        .route(
            "/api/clusters/merge",
            routing::post(finance_manager::api::clusters::handle_post_merge),
        )
        .with_state(db)
        .layer(middleware::from_fn(
            move |mut req: Request<Body>, next: middleware::Next| {
                let user = user.clone();
                async move {
                    req.extensions_mut().insert(user);
                    next.run(req).await
                }
            },
        ))
}

async fn fetch_json(app: Router, uri: &str) -> (StatusCode, Value) {
    let req = Request::builder().uri(uri).body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, json)
}

async fn post_json(app: Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

#[tokio::test]
async fn clusters_groups_similar_products_above_threshold() {
    let t = common::TestDb::new().await;
    let owner_id = Uuid::new_v4();
    do_import(&t, owner_id).await;

    let app = build_test_router(Arc::clone(&t.db), owner_id);
    let (status, json) =
        fetch_json(app, "/api/clusters?scope=product&threshold=0.4").await;
    assert_eq!(status, StatusCode::OK);

    let clusters = json["clusters"].as_array().unwrap();
    assert!(
        !clusters.is_empty(),
        "골든 데이터에는 비슷한 제품 묶음이 최소 1개는 있어야 함. got={json}"
    );
    // 적어도 한 클러스터는 멤버 ≥ 2
    assert!(clusters.iter().all(|c| c["members"].as_array().unwrap().len() >= 2));
}
```

- [ ] **Step 2: 테스트 실행해서 컴파일 실패 확인**

Run:
```bash
DATABASE_URL=postgres://app:app@localhost:5432/postgres \
  cargo test -p finance-manager --test test_clusters -- clusters_groups_similar_products_above_threshold
```
Expected: 컴파일 실패 — `finance_manager::api::clusters` 모듈 없음.

- [ ] **Step 3: clusters.rs 모듈 골격 + 계산 함수 작성**

Create `server/src/api/clusters.rs`:

```rust
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, FromQueryResult, Statement,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::ExtractUser;
use crate::error::{AppError, AppResult};

const MAX_CLUSTERS: usize = 200;
const DEFAULT_THRESHOLD: f32 = 0.5;

// ── Request / Response types ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ClustersQuery {
    pub scope: String,                 // "product" | "merchant"
    pub threshold: Option<f32>,        // 0.3 ~ 0.9
}

#[derive(Debug, Serialize)]
pub struct ClusterMember {
    pub id: Uuid,
    pub name: String,
    pub txn_count: i64,
    pub latest_seen: Option<chrono::NaiveDate>,
}

#[derive(Debug, Serialize)]
pub struct Cluster {
    pub members: Vec<ClusterMember>,
    pub suggested_canonical_id: Uuid,
    pub avg_similarity: f32,
}

#[derive(Debug, Serialize)]
pub struct ClustersResponse {
    pub scope: String,
    pub threshold: f32,
    pub clusters: Vec<Cluster>,
    pub truncated: bool,
}

// ── DB row types (raw SQL) ───────────────────────────────────────────────────

#[derive(Debug, FromQueryResult)]
struct PairRow {
    a_id: Uuid,
    b_id: Uuid,
    sim: f32,
}

#[derive(Debug, FromQueryResult)]
struct MemberRow {
    id: Uuid,
    name: String,
    txn_count: i64,
    latest_seen: Option<chrono::NaiveDate>,
}

// ── Public scope enum ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    Product,
    Merchant,
}

impl Scope {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "product" => Some(Self::Product),
            "merchant" => Some(Self::Merchant),
            _ => None,
        }
    }
    fn entity_table(self) -> &'static str {
        match self { Self::Product => "products", Self::Merchant => "merchants" }
    }
    fn fk_column(self) -> &'static str {
        match self { Self::Product => "product_id", Self::Merchant => "merchant_id" }
    }
}

// ── Cluster computation ──────────────────────────────────────────────────────

pub(crate) async fn compute_clusters(
    db: &DatabaseConnection,
    owner_id: Uuid,
    scope: Scope,
    threshold: f32,
) -> Result<(Vec<Cluster>, bool), sea_orm::DbErr> {
    let table = scope.entity_table();

    // 1. 페어 추출 (GIN trgm 인덱스 사용)
    let pair_sql = format!(
        r#"
        SELECT a.id AS a_id, b.id AS b_id,
               similarity(a.name, b.name)::real AS sim
          FROM {table} a
          JOIN {table} b
            ON a.owner_id = b.owner_id
           AND a.id < b.id
           AND a.name % b.name
         WHERE a.owner_id = $1
           AND similarity(a.name, b.name) >= $2
        "#
    );
    let pairs: Vec<PairRow> = PairRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        &pair_sql,
        [owner_id.into(), threshold.into()],
    ))
    .all(db)
    .await?;

    if pairs.is_empty() {
        return Ok((Vec::new(), false));
    }

    // 2. union-find 로 컴포넌트 묶음
    let mut uf: HashMap<Uuid, Uuid> = HashMap::new();
    fn find(uf: &mut HashMap<Uuid, Uuid>, x: Uuid) -> Uuid {
        let parent = *uf.get(&x).unwrap_or(&x);
        if parent == x { return x; }
        let root = find(uf, parent);
        uf.insert(x, root);
        root
    }
    fn union(uf: &mut HashMap<Uuid, Uuid>, a: Uuid, b: Uuid) {
        let ra = find(uf, a);
        let rb = find(uf, b);
        if ra != rb { uf.insert(ra, rb); }
    }
    for p in &pairs {
        uf.entry(p.a_id).or_insert(p.a_id);
        uf.entry(p.b_id).or_insert(p.b_id);
        union(&mut uf, p.a_id, p.b_id);
    }

    let mut groups: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    let ids: Vec<Uuid> = uf.keys().copied().collect();
    for id in ids {
        let root = find(&mut uf, id);
        groups.entry(root).or_default().push(id);
    }

    // 3. 평균 유사도 계산: 각 컴포넌트의 페어들 평균
    let mut sim_acc: HashMap<Uuid, (f32, u32)> = HashMap::new();
    for p in &pairs {
        let root = find(&mut uf, p.a_id);
        let e = sim_acc.entry(root).or_insert((0.0, 0));
        e.0 += p.sim;
        e.1 += 1;
    }

    // 4. 모든 멤버 id 의 메타 (txn_count, latest_seen) 한 번에 조회
    let all_ids: Vec<Uuid> = groups
        .values()
        .filter(|m| m.len() >= 2)
        .flatten()
        .copied()
        .collect();
    if all_ids.is_empty() {
        return Ok((Vec::new(), false));
    }
    let fk = scope.fk_column();
    let meta_sql = format!(
        r#"
        SELECT e.id, e.name,
               COALESCE(s.cnt, 0)::bigint AS txn_count,
               s.latest_seen
          FROM {table} e
          LEFT JOIN (
            SELECT {fk} AS eid, COUNT(*) AS cnt, MAX(occurred_on) AS latest_seen
              FROM transactions
             WHERE owner_id = $1 AND {fk} = ANY($2)
             GROUP BY {fk}
          ) s ON s.eid = e.id
         WHERE e.owner_id = $1 AND e.id = ANY($2)
        "#
    );
    let metas: Vec<MemberRow> = MemberRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        &meta_sql,
        [owner_id.into(), all_ids.clone().into()],
    ))
    .all(db)
    .await?;
    let meta_by_id: HashMap<Uuid, MemberRow> =
        metas.into_iter().map(|m| (m.id, m)).collect();

    // 5. 클러스터 빌드
    let mut clusters: Vec<Cluster> = Vec::new();
    for (root, ids) in groups {
        if ids.len() < 2 { continue; }
        let mut members: Vec<ClusterMember> = ids
            .iter()
            .filter_map(|id| meta_by_id.get(id))
            .map(|m| ClusterMember {
                id: m.id,
                name: m.name.clone(),
                txn_count: m.txn_count,
                latest_seen: m.latest_seen,
            })
            .collect();
        if members.len() < 2 { continue; }
        // 정렬: txn_count desc, name asc (대표 선택용)
        members.sort_by(|a, b| {
            b.txn_count.cmp(&a.txn_count).then_with(|| a.name.cmp(&b.name))
        });
        let suggested_canonical_id = members[0].id;
        let (sim_sum, sim_n) = sim_acc.get(&root).copied().unwrap_or((0.0, 0));
        let avg_similarity = if sim_n == 0 { 0.0 } else { sim_sum / sim_n as f32 };
        clusters.push(Cluster { members, suggested_canonical_id, avg_similarity });
    }
    // 클러스터 정렬: 멤버 수 내림차순
    clusters.sort_by(|a, b| b.members.len().cmp(&a.members.len()));

    let truncated = clusters.len() > MAX_CLUSTERS;
    if truncated { clusters.truncate(MAX_CLUSTERS); }

    Ok((clusters, truncated))
}

// ── GET /api/clusters handler ────────────────────────────────────────────────

pub async fn handle_get_clusters(
    State(db): State<Arc<DatabaseConnection>>,
    ExtractUser(user): ExtractUser,
    Query(q): Query<ClustersQuery>,
) -> AppResult<Json<ClustersResponse>> {
    let scope = Scope::parse(&q.scope).ok_or_else(|| {
        AppError::BadRequest("scope must be 'product' or 'merchant'".into())
    })?;
    let threshold = q.threshold.unwrap_or(DEFAULT_THRESHOLD);
    if !(0.3..=0.9).contains(&threshold) {
        return Err(AppError::BadRequest("threshold must be in [0.3, 0.9]".into()));
    }
    let (clusters, truncated) = compute_clusters(&db, user.sub, scope, threshold).await?;
    Ok(Json(ClustersResponse {
        scope: q.scope,
        threshold,
        clusters,
        truncated,
    }))
}

// ── POST /api/clusters/merge — placeholder (Task 4) ──────────────────────────

#[derive(Debug, Deserialize)]
pub struct MergeRequest {
    pub scope: String,
    pub canonical_id: Uuid,
    pub absorb_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct MergeResponse {
    pub merged_count: usize,
    pub txn_relinked: u64,
    pub aliases_deleted: u64,
}

pub async fn handle_post_merge(
    State(_db): State<Arc<DatabaseConnection>>,
    ExtractUser(_user): ExtractUser,
    Json(_body): Json<MergeRequest>,
) -> Result<Json<MergeResponse>, (StatusCode, String)> {
    // Implemented in Task 4
    Err((StatusCode::NOT_IMPLEMENTED, "implemented in Task 4".into()))
}
```

- [ ] **Step 4: api/mod.rs 에 모듈만 등록 (라우트 등록은 Task 5)**

Edit `server/src/api/mod.rs` 첫 번째 module 선언 블록에 추가:

```rust
pub mod aliases;
pub mod categories;
pub mod clusters;       // ← 추가
pub mod export;
```

- [ ] **Step 5: 컴파일 + Task 2 의 첫 테스트 통과 확인**

Run:
```bash
DATABASE_URL=postgres://app:app@localhost:5432/postgres \
  cargo test -p finance-manager --test test_clusters -- clusters_groups_similar_products_above_threshold
```
Expected: PASS. (다른 테스트는 아직 없음.)

- [ ] **Step 6: 추가 테스트 — singleton 제외, owner 격리, threshold 필터, hint 검증**

Append to `server/tests/test_clusters.rs`:

```rust
use sea_orm::Set as SetVal;
use finance_manager::entity::{merchants, prelude::Merchants, products, prelude::Products};

async fn insert_product(db: &DatabaseConnection, owner_id: Uuid, name: &str) -> Uuid {
    let m = Products::insert(products::ActiveModel {
        id: SetVal(Uuid::new_v4()),
        owner_id: SetVal(owner_id),
        merchant_id: SetVal(None),
        name: SetVal(name.into()),
        review_state: SetVal("confirmed".into()),
    })
    .exec(db)
    .await
    .unwrap();
    m.last_insert_id
}

async fn insert_merchant(db: &DatabaseConnection, owner_id: Uuid, name: &str) -> Uuid {
    let m = Merchants::insert(merchants::ActiveModel {
        id: SetVal(Uuid::new_v4()),
        owner_id: SetVal(owner_id),
        name: SetVal(name.into()),
        review_state: SetVal("confirmed".into()),
    })
    .exec(db)
    .await
    .unwrap();
    m.last_insert_id
}

#[tokio::test]
async fn clusters_excludes_singletons() {
    let t = common::TestDb::new().await;
    let owner_id = Uuid::new_v4();
    insert_product(&t.db, owner_id, "오로지 혼자인 제품").await;

    let app = build_test_router(Arc::clone(&t.db), owner_id);
    let (status, json) =
        fetch_json(app, "/api/clusters?scope=product&threshold=0.3").await;
    assert_eq!(status, StatusCode::OK);
    assert!(json["clusters"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn clusters_respects_owner_isolation() {
    let t = common::TestDb::new().await;
    let owner_a = Uuid::new_v4();
    let owner_b = Uuid::new_v4();
    insert_product(&t.db, owner_a, "고덕방 아이스아메리카노").await;
    insert_product(&t.db, owner_a, "고덕방 아메리카노").await;
    insert_product(&t.db, owner_b, "고덕방 아이스아메리카노").await;
    insert_product(&t.db, owner_b, "고덕방 아메리카노").await;

    let app = build_test_router(Arc::clone(&t.db), owner_a);
    let (status, json) =
        fetch_json(app, "/api/clusters?scope=product&threshold=0.4").await;
    assert_eq!(status, StatusCode::OK);
    let clusters = json["clusters"].as_array().unwrap();
    // owner_a 의 두 row 만 묶이고 owner_b 는 영향 X
    assert_eq!(clusters.len(), 1);
    let members = clusters[0]["members"].as_array().unwrap();
    assert_eq!(members.len(), 2);
}

#[tokio::test]
async fn clusters_threshold_filter_works() {
    let t = common::TestDb::new().await;
    let owner_id = Uuid::new_v4();
    insert_product(&t.db, owner_id, "고덕방 아메리카노").await;
    insert_product(&t.db, owner_id, "전혀 다른 제품 이름").await;

    let app = build_test_router(Arc::clone(&t.db), owner_id);
    let (status, json) =
        fetch_json(app, "/api/clusters?scope=product&threshold=0.9").await;
    assert_eq!(status, StatusCode::OK);
    assert!(json["clusters"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn clusters_works_for_merchant_scope() {
    let t = common::TestDb::new().await;
    let owner_id = Uuid::new_v4();
    insert_merchant(&t.db, owner_id, "스타벅스 고덕점").await;
    insert_merchant(&t.db, owner_id, "스타벅스 고덕").await;

    let app = build_test_router(Arc::clone(&t.db), owner_id);
    let (status, json) =
        fetch_json(app, "/api/clusters?scope=merchant&threshold=0.4").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["clusters"].as_array().unwrap().len(), 1);
}
```

- [ ] **Step 7: 모든 cluster 테스트 실행 + 통과 확인**

Run:
```bash
DATABASE_URL=postgres://app:app@localhost:5432/postgres \
  cargo test -p finance-manager --test test_clusters
```
Expected: 5개 테스트 모두 PASS.

- [ ] **Step 8: Commit**

```bash
git add server/src/api/clusters.rs server/src/api/mod.rs server/tests/test_clusters.rs
git commit -m "feat(api): GET /api/clusters using pg_trgm union-find"
```

---

## Task 3: Backend — `POST /api/clusters/merge` (TDD)

**Files:**
- Modify: `server/src/api/clusters.rs` (`handle_post_merge` 본체 구현)
- Modify: `server/tests/test_clusters.rs` (병합 테스트 추가)

- [ ] **Step 1: 병합 테스트 추가 (failing)**

Append to `server/tests/test_clusters.rs`:

```rust
use serde_json::json;
use finance_manager::entity::{aliases, prelude::Aliases, transactions, prelude::Transactions};
use sea_orm::{ColumnTrait, QueryFilter};

#[tokio::test]
async fn merge_relinks_transactions_and_deletes_absorbed() {
    let t = common::TestDb::new().await;
    let owner_id = Uuid::new_v4();
    do_import(&t, owner_id).await;

    // 골든 데이터에서 같은 가맹점의 두 product 잡기
    // (어느 쌍이든 cluster에 잡힌 것 중 하나 사용)
    let app = build_test_router(Arc::clone(&t.db), owner_id);
    let (_, list) = fetch_json(app, "/api/clusters?scope=product&threshold=0.4").await;
    let clusters = list["clusters"].as_array().unwrap();
    assert!(!clusters.is_empty());
    let first = &clusters[0];
    let canonical_id: Uuid = serde_json::from_value(first["suggested_canonical_id"].clone()).unwrap();
    let absorb_ids: Vec<Uuid> = first["members"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| {
            let id: Uuid = serde_json::from_value(m["id"].clone()).ok()?;
            (id != canonical_id).then_some(id)
        })
        .collect();
    assert!(!absorb_ids.is_empty());

    // 병합 전 transaction 수 측정
    let before_canonical: u64 = Transactions::find()
        .filter(transactions::Column::OwnerId.eq(owner_id))
        .filter(transactions::Column::ProductId.eq(canonical_id))
        .count(&*t.db).await.unwrap();
    let mut before_absorbed: u64 = 0;
    for id in &absorb_ids {
        before_absorbed += Transactions::find()
            .filter(transactions::Column::OwnerId.eq(owner_id))
            .filter(transactions::Column::ProductId.eq(*id))
            .count(&*t.db).await.unwrap();
    }

    let app = build_test_router(Arc::clone(&t.db), owner_id);
    let (status, json) = post_json(app, "/api/clusters/merge", json!({
        "scope": "product",
        "canonical_id": canonical_id,
        "absorb_ids": absorb_ids,
    })).await;
    assert_eq!(status, StatusCode::OK, "merge: {json}");
    assert_eq!(json["merged_count"].as_u64(), Some(absorb_ids.len() as u64));

    // 병합 후 absorb row 모두 사라짐
    for id in &absorb_ids {
        let still = Products::find_by_id(*id).one(&*t.db).await.unwrap();
        assert!(still.is_none(), "absorbed product {id} should be deleted");
    }
    // canonical 의 transaction 수 = 이전 합계
    let after_canonical: u64 = Transactions::find()
        .filter(transactions::Column::OwnerId.eq(owner_id))
        .filter(transactions::Column::ProductId.eq(canonical_id))
        .count(&*t.db).await.unwrap();
    assert_eq!(after_canonical, before_canonical + before_absorbed);
}

#[tokio::test]
async fn merge_deletes_aliases_pointing_to_absorbed() {
    let t = common::TestDb::new().await;
    let owner_id = Uuid::new_v4();

    let canonical = insert_product(&t.db, owner_id, "고덕방 아이스아메리카노").await;
    let absorb = insert_product(&t.db, owner_id, "고덕방 아메리카노").await;
    // alias 한 개 등록
    Aliases::insert(aliases::ActiveModel {
        id: SetVal(Uuid::new_v4()),
        owner_id: SetVal(owner_id),
        scope: SetVal("product".into()),
        raw_text: SetVal("고덕방 아메리카노".into()),
        norm_key: SetVal("고덕방 아메리카노".into()),
        target_id: SetVal(absorb),
    }).exec(&*t.db).await.unwrap();

    let app = build_test_router(Arc::clone(&t.db), owner_id);
    let (status, json) = post_json(app, "/api/clusters/merge", json!({
        "scope": "product",
        "canonical_id": canonical,
        "absorb_ids": [absorb],
    })).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert!(json["aliases_deleted"].as_u64().unwrap() >= 1);

    let remaining = Aliases::find()
        .filter(aliases::Column::TargetId.eq(absorb))
        .count(&*t.db).await.unwrap();
    assert_eq!(remaining, 0);
}

#[tokio::test]
async fn merge_rejects_canonical_in_absorb_ids() {
    let t = common::TestDb::new().await;
    let owner_id = Uuid::new_v4();
    let id = insert_product(&t.db, owner_id, "X").await;

    let app = build_test_router(Arc::clone(&t.db), owner_id);
    let (status, _) = post_json(app, "/api/clusters/merge", json!({
        "scope": "product",
        "canonical_id": id,
        "absorb_ids": [id],
    })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn merge_rejects_empty_absorb_ids() {
    let t = common::TestDb::new().await;
    let owner_id = Uuid::new_v4();
    let id = insert_product(&t.db, owner_id, "X").await;

    let app = build_test_router(Arc::clone(&t.db), owner_id);
    let (status, _) = post_json(app, "/api/clusters/merge", json!({
        "scope": "product",
        "canonical_id": id,
        "absorb_ids": [],
    })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
```

추가 import 도 필요:
```rust
use sea_orm::PaginatorTrait;
```

- [ ] **Step 2: 테스트 실행해서 503/501 실패 확인**

Run:
```bash
DATABASE_URL=postgres://app:app@localhost:5432/postgres \
  cargo test -p finance-manager --test test_clusters merge_
```
Expected: 4개 테스트 모두 FAIL — `handle_post_merge` 가 NOT_IMPLEMENTED 반환 또는 컴파일 에러.

- [ ] **Step 3: handle_post_merge 본체 구현**

Replace `handle_post_merge` 와 그 placeholder 구역 in `server/src/api/clusters.rs`:

```rust
use sea_orm::{ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, TransactionTrait};
use crate::entity::{aliases, prelude::Aliases, prelude::Merchants, prelude::Products, prelude::Transactions, transactions};

pub async fn handle_post_merge(
    State(db): State<Arc<DatabaseConnection>>,
    ExtractUser(user): ExtractUser,
    Json(body): Json<MergeRequest>,
) -> AppResult<Json<MergeResponse>> {
    let scope = Scope::parse(&body.scope).ok_or_else(|| {
        AppError::BadRequest("scope must be 'product' or 'merchant'".into())
    })?;
    if body.absorb_ids.is_empty() {
        return Err(AppError::BadRequest("absorb_ids must not be empty".into()));
    }
    if body.absorb_ids.iter().any(|id| *id == body.canonical_id) {
        return Err(AppError::BadRequest(
            "canonical_id must not be in absorb_ids".into(),
        ));
    }

    let owner_id = user.sub;
    let txn = db.begin().await?;

    // 1. canonical 검증 + lock absorb rows
    let lock_table = scope.entity_table();
    let lock_sql = format!(
        "SELECT id FROM {lock_table} WHERE owner_id = $1 AND id = ANY($2) FOR UPDATE"
    );
    txn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        &lock_sql,
        [owner_id.into(), body.absorb_ids.clone().into()],
    ))
    .await?;

    // 2. transactions 재배선
    let fk = scope.fk_column();
    let upd_sql = format!(
        "UPDATE transactions SET {fk} = $1 \
         WHERE owner_id = $2 AND {fk} = ANY($3)"
    );
    let upd_res = txn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        &upd_sql,
        [
            body.canonical_id.into(),
            owner_id.into(),
            body.absorb_ids.clone().into(),
        ],
    )).await?;
    let txn_relinked = upd_res.rows_affected();

    // 3. aliases 삭제 (흡수 대상 가리키던 것)
    let alias_scope = match scope { Scope::Product => "product", Scope::Merchant => "merchant" };
    let alias_del = Aliases::delete_many()
        .filter(aliases::Column::OwnerId.eq(owner_id))
        .filter(aliases::Column::Scope.eq(alias_scope))
        .filter(aliases::Column::TargetId.is_in(body.absorb_ids.clone()))
        .exec(&txn)
        .await?;
    let aliases_deleted = alias_del.rows_affected;

    // 4. absorbed entity 삭제
    match scope {
        Scope::Product => {
            Products::delete_many()
                .filter(crate::entity::products::Column::OwnerId.eq(owner_id))
                .filter(crate::entity::products::Column::Id.is_in(body.absorb_ids.clone()))
                .exec(&txn)
                .await?;
        }
        Scope::Merchant => {
            Merchants::delete_many()
                .filter(crate::entity::merchants::Column::OwnerId.eq(owner_id))
                .filter(crate::entity::merchants::Column::Id.is_in(body.absorb_ids.clone()))
                .exec(&txn)
                .await?;
        }
    }

    txn.commit().await?;
    Ok(Json(MergeResponse {
        merged_count: body.absorb_ids.len(),
        txn_relinked,
        aliases_deleted,
    }))
}
```

`Transactions` import 가 사용되지 않으면 prelude import 에서 빼고, 위 use 블록의 미사용 import 들을 정리한다.

- [ ] **Step 4: 테스트 실행해서 통과 확인**

Run:
```bash
DATABASE_URL=postgres://app:app@localhost:5432/postgres \
  cargo test -p finance-manager --test test_clusters
```
Expected: 9개 테스트 (이전 5 + 새 4) 모두 PASS.

- [ ] **Step 5: 전체 백엔드 회귀**

Run:
```bash
DATABASE_URL=postgres://app:app@localhost:5432/postgres cargo test -p finance-manager
```
Expected: 기존 87개 + 새 9개 = 96개 모두 PASS.

- [ ] **Step 6: Commit**

```bash
git add server/src/api/clusters.rs server/tests/test_clusters.rs
git commit -m "feat(api): POST /api/clusters/merge with FOR UPDATE + alias cleanup"
```

---

## Task 4: Backend — 라우트 등록 (`mod.rs`)

**Files:**
- Modify: `server/src/api/mod.rs`

- [ ] **Step 1: 라우트 두 개 등록**

`server/src/api/mod.rs` 의 `protected = Router::new()` 체인 안, M2 라우트 옆에 추가:

```rust
        // Bulk cluster merge (2026-05-11)
        .route("/api/clusters", get(clusters::handle_get_clusters))
        .route("/api/clusters/merge", post(clusters::handle_post_merge))
```

- [ ] **Step 2: 빌드 확인**

Run:
```bash
cargo build -p finance-manager
```
Expected: 성공.

- [ ] **Step 3: 백엔드 전체 테스트 다시 실행 (라우터에서도 동작하는지)**

Run:
```bash
DATABASE_URL=postgres://app:app@localhost:5432/postgres cargo test -p finance-manager
```
Expected: 모두 PASS.

- [ ] **Step 4: Commit**

```bash
git add server/src/api/mod.rs
git commit -m "feat(router): wire /api/clusters routes into protected router"
```

---

## Task 5: Frontend — zod 스키마 + 헬퍼 (TDD)

**Files:**
- Modify: `web/lib/schemas.ts`
- Create: `web/lib/cluster-data.ts`
- Create: `web/__tests__/clusters.test.tsx` (헬퍼 테스트만 본 task)

- [ ] **Step 1: 헬퍼 테스트 작성 (failing)**

Create `web/__tests__/clusters.test.tsx`:

```tsx
import { describe, it, expect } from "vitest";
import {
  pickDefaultCanonical,
  sortMembersForDisplay,
  formatLatestSeen,
} from "@/lib/cluster-data";

const m = (id: string, name: string, txn_count = 0, latest_seen: string | null = null) => ({
  id, name, txn_count, latest_seen,
});

describe("cluster-data helpers", () => {
  it("pickDefaultCanonical: 트랜잭션 수가 가장 많은 멤버를 고른다", () => {
    const members = [m("a", "A", 1), m("b", "B", 5), m("c", "C", 3)];
    expect(pickDefaultCanonical(members)).toBe("b");
  });

  it("pickDefaultCanonical: 동률이면 가나다순 첫 번째", () => {
    const members = [m("b", "Bravo", 3), m("a", "Alpha", 3)];
    expect(pickDefaultCanonical(members)).toBe("a");
  });

  it("sortMembersForDisplay: 트랜잭션 수 내림차순", () => {
    const members = [m("a", "A", 1), m("b", "B", 5), m("c", "C", 3)];
    const sorted = sortMembersForDisplay(members);
    expect(sorted.map(s => s.id)).toEqual(["b", "c", "a"]);
  });

  it("formatLatestSeen: 날짜 문자열을 YYYY-MM-DD 그대로 표시, null 은 dash", () => {
    expect(formatLatestSeen("2026-02-28")).toBe("2026-02-28");
    expect(formatLatestSeen(null)).toBe("—");
  });
});
```

- [ ] **Step 2: 테스트 실패 확인**

Run:
```bash
cd web && npm test -- clusters.test.tsx
```
Expected: FAIL — `@/lib/cluster-data` 모듈 없음.

- [ ] **Step 3: 헬퍼 작성**

Create `web/lib/cluster-data.ts`:

```ts
export type ClusterMemberView = {
  id: string;
  name: string;
  txn_count: number;
  latest_seen: string | null;
};

/** 트랜잭션 수 최댓값 멤버를 대표로. 동률 시 name 가나다순 첫 번째. */
export function pickDefaultCanonical(members: ClusterMemberView[]): string {
  const sorted = [...members].sort((a, b) => {
    if (b.txn_count !== a.txn_count) return b.txn_count - a.txn_count;
    return a.name.localeCompare(b.name, "ko");
  });
  return sorted[0]!.id;
}

/** 표시용 정렬: 트랜잭션 수 내림차순, 동률 시 가나다순. */
export function sortMembersForDisplay(members: ClusterMemberView[]): ClusterMemberView[] {
  return [...members].sort((a, b) => {
    if (b.txn_count !== a.txn_count) return b.txn_count - a.txn_count;
    return a.name.localeCompare(b.name, "ko");
  });
}

export function formatLatestSeen(date: string | null): string {
  return date ?? "—";
}
```

- [ ] **Step 4: zod 스키마 추가**

Append to `web/lib/schemas.ts`:

```ts
// ── Clusters (2026-05-11) ─────────────────────────────────────────────────────

export const ClusterMemberSchema = z.object({
  id: z.string().uuid(),
  name: z.string(),
  txn_count: z.number().int(),
  latest_seen: z.string().nullable(),
});
export type ClusterMember = z.infer<typeof ClusterMemberSchema>;

export const ClusterSchema = z.object({
  members: z.array(ClusterMemberSchema),
  suggested_canonical_id: z.string().uuid(),
  avg_similarity: z.number(),
});
export type Cluster = z.infer<typeof ClusterSchema>;

export const ClustersResponseSchema = z.object({
  scope: z.enum(["product", "merchant"]),
  threshold: z.number(),
  clusters: z.array(ClusterSchema),
  truncated: z.boolean(),
});
export type ClustersResponse = z.infer<typeof ClustersResponseSchema>;

export const MergeResponseSchema = z.object({
  merged_count: z.number().int(),
  txn_relinked: z.number().int(),
  aliases_deleted: z.number().int(),
});
export type MergeResponse = z.infer<typeof MergeResponseSchema>;
```

- [ ] **Step 5: 테스트 통과 확인**

Run:
```bash
cd web && npm test -- clusters.test.tsx
```
Expected: 4개 PASS.

- [ ] **Step 6: 전체 프런트 테스트 회귀**

Run:
```bash
cd web && npm test
```
Expected: 기존 + 새 4 PASS.

- [ ] **Step 7: Commit**

```bash
git add web/lib/schemas.ts web/lib/cluster-data.ts web/__tests__/clusters.test.tsx
git commit -m "feat(web): cluster zod schemas + display helpers"
```

---

## Task 6: Frontend — `cluster-card` 컴포넌트 (TDD)

**Files:**
- Create: `web/components/cluster-card.tsx`
- Modify: `web/__tests__/clusters.test.tsx` (컴포넌트 테스트 추가)

- [ ] **Step 1: 컴포넌트 테스트 추가 (failing)**

Append to `web/__tests__/clusters.test.tsx`:

```tsx
import { render, screen, fireEvent } from "@testing-library/react";
import { ClusterCard } from "@/components/cluster-card";

const sampleCluster = {
  members: [
    { id: "a", name: "고덕방 아이스아메리카노", txn_count: 6, latest_seen: "2026-02-28" },
    { id: "b", name: "고덕방 아메리카노",       txn_count: 2, latest_seen: "2026-02-15" },
    { id: "c", name: "고덕방 아아",             txn_count: 1, latest_seen: "2026-02-10" },
  ],
  suggested_canonical_id: "a",
  avg_similarity: 0.62,
};

describe("ClusterCard", () => {
  it("멤버를 트랜잭션 수 내림차순으로 렌더하고 최댓값을 라디오로 선택", () => {
    render(<ClusterCard cluster={sampleCluster} onMerge={() => {}} />);
    const rows = screen.getAllByRole("row");
    // 행 첫 번째 = txn 6 (a)
    expect(rows[0]).toHaveTextContent("고덕방 아이스아메리카노");
    expect(screen.getByLabelText("대표: 고덕방 아이스아메리카노")).toBeChecked();
  });

  it("대표로 선택된 row 의 흡수 체크박스는 disabled", () => {
    render(<ClusterCard cluster={sampleCluster} onMerge={() => {}} />);
    const cb = screen.getByLabelText("흡수: 고덕방 아이스아메리카노") as HTMLInputElement;
    expect(cb.disabled).toBe(true);
  });

  it("흡수 0개면 병합 버튼 disabled", () => {
    render(<ClusterCard cluster={sampleCluster} onMerge={() => {}} />);
    // 기본은 나머지 흡수 ON. 두 흡수 체크 해제.
    fireEvent.click(screen.getByLabelText("흡수: 고덕방 아메리카노"));
    fireEvent.click(screen.getByLabelText("흡수: 고덕방 아아"));
    expect(screen.getByRole("button", { name: /병합/ })).toBeDisabled();
  });

  it("병합 버튼 클릭 시 onMerge(canonical_id, absorb_ids) 호출", () => {
    const onMerge = vi.fn();
    render(<ClusterCard cluster={sampleCluster} onMerge={onMerge} />);
    fireEvent.click(screen.getByRole("button", { name: /병합/ }));
    expect(onMerge).toHaveBeenCalledWith("a", expect.arrayContaining(["b", "c"]));
  });
});
```

상단 imports 에도 `vi` 가 필요하면 `vitest` 에서 import 추가.

- [ ] **Step 2: 테스트 실패 확인**

Run:
```bash
cd web && npm test -- clusters.test.tsx
```
Expected: 컴포넌트 import 실패로 FAIL.

- [ ] **Step 3: ClusterCard 작성**

Create `web/components/cluster-card.tsx`:

```tsx
"use client";

import { useMemo, useState } from "react";
import { Card, CardContent, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import {
  pickDefaultCanonical,
  sortMembersForDisplay,
  formatLatestSeen,
} from "@/lib/cluster-data";
import type { Cluster } from "@/lib/schemas";

type Props = {
  cluster: Cluster;
  onMerge: (canonicalId: string, absorbIds: string[]) => void;
};

export function ClusterCard({ cluster, onMerge }: Props) {
  const sorted = useMemo(() => sortMembersForDisplay(cluster.members), [cluster.members]);
  const [canonicalId, setCanonicalId] = useState<string>(
    cluster.suggested_canonical_id || pickDefaultCanonical(cluster.members)
  );
  const [absorb, setAbsorb] = useState<Set<string>>(
    () => new Set(sorted.filter(m => m.id !== cluster.suggested_canonical_id).map(m => m.id))
  );

  // canonical 변경 시 그 멤버는 흡수에서 제외
  const onPickCanonical = (id: string) => {
    setCanonicalId(id);
    setAbsorb(prev => {
      const next = new Set(prev);
      next.delete(id);
      return next;
    });
  };

  const toggleAbsorb = (id: string) => {
    setAbsorb(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });
  };

  const absorbList = [...absorb];
  const mergeDisabled = absorbList.length === 0;

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">
          {sorted.length}개 후보 · 평균 유사도 {(cluster.avg_similarity * 100).toFixed(0)}%
        </CardTitle>
      </CardHeader>
      <CardContent className="p-0">
        <table className="w-full text-sm">
          <tbody>
            {sorted.map(member => {
              const isCanonical = member.id === canonicalId;
              return (
                <tr key={member.id} className="border-t">
                  <td className="px-3 py-2">
                    <input
                      type="radio"
                      name={`canonical-${cluster.suggested_canonical_id}`}
                      aria-label={`대표: ${member.name}`}
                      checked={isCanonical}
                      onChange={() => onPickCanonical(member.id)}
                    />
                  </td>
                  <td className="px-3 py-2">
                    <input
                      type="checkbox"
                      aria-label={`흡수: ${member.name}`}
                      checked={absorb.has(member.id)}
                      disabled={isCanonical}
                      onChange={() => toggleAbsorb(member.id)}
                    />
                  </td>
                  <td className="px-3 py-2 font-medium">{member.name}</td>
                  <td className="px-3 py-2 text-right text-muted-foreground">
                    거래 {member.txn_count}건
                  </td>
                  <td className="px-3 py-2 text-right text-muted-foreground">
                    최근 {formatLatestSeen(member.latest_seen)}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </CardContent>
      <CardFooter className="justify-end">
        <Button
          disabled={mergeDisabled}
          onClick={() => onMerge(canonicalId, absorbList)}
        >
          병합
        </Button>
      </CardFooter>
    </Card>
  );
}
```

- [ ] **Step 4: 테스트 통과 확인**

Run:
```bash
cd web && npm test -- clusters.test.tsx
```
Expected: 헬퍼 4 + 컴포넌트 4 = 8개 PASS.

- [ ] **Step 5: Commit**

```bash
git add web/components/cluster-card.tsx web/__tests__/clusters.test.tsx
git commit -m "feat(web): ClusterCard component with radio/checkbox/merge UX"
```

---

## Task 7: Frontend — `cluster-tab` 컨테이너 (TDD)

**Files:**
- Create: `web/components/cluster-tab.tsx`
- Modify: `web/__tests__/clusters.test.tsx`

- [ ] **Step 1: 컨테이너 테스트 추가 (failing)**

Append to `web/__tests__/clusters.test.tsx`:

```tsx
import { ClusterTab } from "@/components/cluster-tab";

// fetch 모킹: 요청별로 응답 설정
function mockFetchSequence(responses: Array<unknown>) {
  let i = 0;
  global.fetch = vi.fn(async () => {
    const body = responses[i++] ?? { clusters: [], scope: "product", threshold: 0.5, truncated: false };
    return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
  }) as unknown as typeof fetch;
}

describe("ClusterTab", () => {
  it("초기에는 안내 텍스트만 보이고 fetch 안 함", () => {
    mockFetchSequence([]);
    render(<ClusterTab />);
    expect(screen.getByText(/다시 계산/)).toBeInTheDocument();
    expect(global.fetch).not.toHaveBeenCalled();
  });

  it("'다시 계산' 클릭 시 fetch 후 카드 렌더", async () => {
    mockFetchSequence([{
      scope: "product", threshold: 0.5, truncated: false,
      clusters: [{
        members: [
          { id: "a", name: "고덕방 아메리카노", txn_count: 3, latest_seen: "2026-02-28" },
          { id: "b", name: "고덕방 아아",       txn_count: 1, latest_seen: "2026-02-15" },
        ],
        suggested_canonical_id: "a",
        avg_similarity: 0.5,
      }],
    }]);
    render(<ClusterTab />);
    fireEvent.click(screen.getByRole("button", { name: /다시 계산/ }));
    expect(await screen.findByText("고덕방 아메리카노")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: 테스트 실패 확인**

Run:
```bash
cd web && npm test -- clusters.test.tsx
```
Expected: ClusterTab import 실패.

- [ ] **Step 3: ClusterTab 작성**

Create `web/components/cluster-tab.tsx`:

```tsx
"use client";

import { useState, useTransition } from "react";
import { Button } from "@/components/ui/button";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { ClusterCard } from "@/components/cluster-card";
import { ClustersResponseSchema, type Cluster } from "@/lib/schemas";

type Scope = "product" | "merchant";

export function ClusterTab() {
  const [scope, setScope] = useState<Scope>("product");
  const [threshold, setThreshold] = useState<number>(0.5);
  const [clusters, setClusters] = useState<Cluster[] | null>(null);
  const [truncated, setTruncated] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pending, startTransition] = useTransition();

  async function recompute() {
    setError(null);
    const r = await fetch(
      `/api/clusters-proxy?scope=${scope}&threshold=${threshold}`,
      { cache: "no-store" }
    );
    if (!r.ok) {
      setError(`Failed to fetch (${r.status})`);
      return;
    }
    const parsed = ClustersResponseSchema.safeParse(await r.json());
    if (!parsed.success) {
      setError("응답 스키마가 올바르지 않습니다.");
      return;
    }
    setClusters(parsed.data.clusters);
    setTruncated(parsed.data.truncated);
  }

  async function merge(canonicalId: string, absorbIds: string[]) {
    const r = await fetch(`/api/clusters-proxy/merge`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ scope, canonical_id: canonicalId, absorb_ids: absorbIds }),
    });
    if (!r.ok) {
      setError(`Merge failed (${r.status})`);
      return;
    }
    startTransition(() => { void recompute(); });
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-3 flex-wrap">
        <Tabs value={scope} onValueChange={(v) => setScope(v as Scope)}>
          <TabsList>
            <TabsTrigger value="product">Products</TabsTrigger>
            <TabsTrigger value="merchant">Merchants</TabsTrigger>
          </TabsList>
        </Tabs>
        <label className="flex items-center gap-2 text-sm">
          임계치 {threshold.toFixed(2)}
          <input
            type="range" min={0.3} max={0.9} step={0.05}
            value={threshold}
            onChange={e => setThreshold(parseFloat(e.target.value))}
          />
        </label>
        <Button onClick={recompute} disabled={pending}>다시 계산</Button>
      </div>

      {error && (
        <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>
      )}
      {truncated && (
        <Alert><AlertDescription>200개 이상 후보가 있어 잘렸습니다. 임계치를 올려보세요.</AlertDescription></Alert>
      )}

      {clusters && clusters.length === 0 && (
        <p className="text-sm text-muted-foreground">묶을 후보가 없습니다.</p>
      )}

      <div className="grid gap-4">
        {clusters?.map((c, i) => (
          <ClusterCard key={`${c.suggested_canonical_id}-${i}`} cluster={c} onMerge={merge} />
        ))}
      </div>
    </div>
  );
}
```

- [ ] **Step 4: Next route handler 프록시 두 개 작성**

Create `web/app/api/clusters-proxy/route.ts`:

```ts
import { NextRequest, NextResponse } from "next/server";
import { apiFetch } from "@/lib/api";
import { ClustersResponseSchema } from "@/lib/schemas";

export async function GET(req: NextRequest) {
  const url = new URL(req.url);
  const scope = url.searchParams.get("scope") ?? "product";
  const threshold = url.searchParams.get("threshold") ?? "0.5";
  try {
    const data = await apiFetch(
      `/api/clusters?scope=${encodeURIComponent(scope)}&threshold=${encodeURIComponent(threshold)}`,
      { schema: ClustersResponseSchema }
    );
    return NextResponse.json(data);
  } catch (err) {
    const message = err instanceof Error ? err.message : "Failed";
    return NextResponse.json({ error: message }, { status: 500 });
  }
}
```

Create `web/app/api/clusters-proxy/merge/route.ts`:

```ts
import { NextRequest, NextResponse } from "next/server";
import { apiFetch } from "@/lib/api";
import { MergeResponseSchema } from "@/lib/schemas";

export async function POST(req: NextRequest) {
  const body = await req.text();
  try {
    const data = await apiFetch(`/api/clusters/merge`, {
      method: "POST",
      body,
      headers: { "Content-Type": "application/json" },
      schema: MergeResponseSchema,
    });
    return NextResponse.json(data);
  } catch (err) {
    const message = err instanceof Error ? err.message : "Failed";
    return NextResponse.json({ error: message }, { status: 500 });
  }
}
```

- [ ] **Step 5: 테스트 통과 확인**

Run:
```bash
cd web && npm test -- clusters.test.tsx
```
Expected: 헬퍼 4 + ClusterCard 4 + ClusterTab 2 = 10개 PASS.

- [ ] **Step 6: Commit**

```bash
git add web/components/cluster-tab.tsx web/app/api/clusters-proxy web/__tests__/clusters.test.tsx
git commit -m "feat(web): ClusterTab container + Next route proxies for /api/clusters"
```

---

## Task 8: Frontend — `/aliases` 페이지에 "클러스터" 탭 통합

**Files:**
- Modify: `web/app/(app)/aliases/page.tsx`

- [ ] **Step 1: TABS 배열에 새 항목 + TabsContent 추가**

Edit `web/app/(app)/aliases/page.tsx` 의 `TABS` 상수:

```tsx
const TABS = [
  { value: "category", label: "Category" },
  { value: "merchant", label: "Merchant" },
  { value: "payment_method", label: "Payment" },
  { value: "product", label: "Product" },
  { value: "cluster", label: "클러스터" },
] as const;
```

같은 파일에서 `TABS.map(...) <TabsContent>` 블록 아래에 `cluster` 케이스를 별도 분기로 추가 (기존 RSC TabPanel 과 다른 클라이언트 컴포넌트라서):

```tsx
import { ClusterTab } from "@/components/cluster-tab";
```

기존 TabsContent map 을 다음처럼 분기:

```tsx
{TABS.map((tab) => (
  <TabsContent key={tab.value} value={tab.value} className="mt-4">
    {tab.value === "cluster" ? (
      <ClusterTab />
    ) : (
      <Suspense
        fallback={
          <div className="flex items-center gap-2 py-8 text-muted-foreground text-sm">
            <Loader2 className="h-4 w-4 animate-spin" />
            Loading {tab.label.toLowerCase()} queue...
          </div>
        }
      >
        <TabPanel scope={tab.value as Exclude<TabScope, "cluster">} />
      </Suspense>
    )}
  </TabsContent>
))}
```

`TabScope` 타입은 이제 `"cluster"` 도 포함하므로, `fetchReviewQueue`/`TabPanel` 가 받는 타입을 좁혀야 한다:

```tsx
type TabScope = (typeof TABS)[number]["value"];
type ReviewScope = Exclude<TabScope, "cluster">;
async function fetchReviewQueue(scope: ReviewScope): Promise<ReviewQueueItem[]> { /* 기존 코드 */ }
async function TabPanel({ scope }: { scope: ReviewScope }) { /* 기존 코드 */ }
```

- [ ] **Step 2: 빌드 + 타입체크**

Run:
```bash
cd web && npm run build
```
Expected: build 성공.

- [ ] **Step 3: 전체 프런트 테스트**

Run:
```bash
cd web && npm test
```
Expected: 기존 + 새 10 PASS.

- [ ] **Step 4: Commit**

```bash
git add web/app/\(app\)/aliases/page.tsx
git commit -m "feat(web): add 클러스터 tab to /aliases page"
```

---

## Task 9: 인수 검증 — 골든 데이터 + 수동 UI 확인

**Files:**
- 없음 (검증만)

- [ ] **Step 1: dev DB fresh + 골든 import**

Run:
```bash
DATABASE_URL=postgres://app:app@localhost:5432/finance \
  cargo run -p migration -- fresh
docker compose up -d server web
```

브라우저에서 `/` 접속해서 `2026년 02월.xlsx` 업로드.

- [ ] **Step 2: 클러스터 탭 동작 확인**

`/aliases` → "클러스터" 탭 → Products → 임계치 0.5 → "다시 계산" 클릭.

Expected: 최소 1개 이상의 클러스터 카드 노출. 멤버는 트랜잭션 수 내림차순, 첫 row 라디오 ON, 나머지 흡수 ON.

- [ ] **Step 3: 병합 동작 확인**

한 클러스터에서 "병합" 클릭.

Expected: 토스트 / 카드 자동 사라짐 또는 목록 갱신. `/price-history` Products 목록에서 해당 product 가 줄어들었는지 확인.

- [ ] **Step 4: Merchants 스코프 토글 확인**

서브탭 Merchants → "다시 계산" → 카드 노출(없으면 빈 상태 메시지).

- [ ] **Step 5: 임계치 0.9 → 다시 계산 → 결과 거의 0**

- [ ] **Step 6: 백엔드 + 프런트 테스트 모두 회귀**

Run:
```bash
DATABASE_URL=postgres://app:app@localhost:5432/postgres cargo test -p finance-manager
cd web && npm test
```
Expected: 전부 PASS.

- [ ] **Step 7: CLAUDE.md 누적 컨텍스트 한 줄 추가**

`CLAUDE.md` 의 "Cumulative Context" 섹션 끝에 한 줄 추가:

```
- 2026-05-11: 일괄 클러스터 병합 — pg_trgm + GIN trgm 인덱스 추가, `server/src/api/clusters.rs` 신규 (`GET /api/clusters`, `POST /api/clusters/merge`), `/aliases` 5번째 "클러스터" 탭. union-find 컴포넌트화 후 사용자가 카드 단위로 대표/흡수 선택 → 단일 트랜잭션 SELECT FOR UPDATE + UPDATE transactions + DELETE aliases + DELETE absorbed entities. alias 학습 보존 X (다음 import 에 같은 raw 가 또 들어오면 신규 entity 재생성). 백엔드 96/96 (+9), 프런트 X/X (+10). Spec/plan: `docs/superpowers/{specs,plans}/2026-05-11-bulk-cluster-merge*`.
```

- [ ] **Step 8: 최종 commit**

```bash
git add CLAUDE.md
git commit -m "docs: add bulk cluster merge to cumulative context"
```

---

## Self-Review Note

- Spec coverage: §3 (아키텍처) → Task 1+2+4+5+8 / §4 (DB) → Task 1 / §5.1 (GET /clusters) → Task 2 / §5.2 (POST /merge) → Task 3 / §6 (Frontend) → Task 5+6+7+8 / §7 (엣지케이스) → Task 2,3 의 검증 테스트 / §8 (테스트 계획) → Task 2,3,5,6,7 / §9 (구현 순서) → Task 1→2→3→4→5→6→7→8→9.
- 모든 step 은 코드/명령/예상 출력 포함. placeholder 없음.
- 타입 일관성: `ClusterMember`, `Cluster`, `MergeResponse` 가 backend 직렬화 ↔ frontend zod 양쪽에서 동일한 필드명 사용.
- Scope: 단일 plan 으로 끝낼 수 있는 크기.
