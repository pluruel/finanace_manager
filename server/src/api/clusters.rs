use axum::{
    extract::{Query, State},
    Json,
};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseBackend, DatabaseConnection, EntityTrait,
    FromQueryResult, QueryFilter, Statement, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::entity::{aliases, merchants, products, prelude::{Aliases, Merchants, Products}};

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

// ── POST /api/clusters/merge — placeholder (Task 3) ──────────────────────────

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

    // 1. Lock absorb rows (race protection via SELECT FOR UPDATE)
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

    // 2. Repoint transactions to canonical
    let fk = scope.fk_column();
    let upd_sql = format!(
        "UPDATE transactions SET {fk} = $1 \
         WHERE owner_id = $2 AND {fk} = ANY($3)"
    );
    let upd_res = txn
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            &upd_sql,
            [
                body.canonical_id.into(),
                owner_id.into(),
                body.absorb_ids.clone().into(),
            ],
        ))
        .await?;
    let txn_relinked = upd_res.rows_affected();

    // 3. Delete aliases pointing at absorbed entities
    let alias_scope = match scope {
        Scope::Product => "product",
        Scope::Merchant => "merchant",
    };
    let alias_del = Aliases::delete_many()
        .filter(aliases::Column::OwnerId.eq(owner_id))
        .filter(aliases::Column::Scope.eq(alias_scope))
        .filter(aliases::Column::TargetId.is_in(body.absorb_ids.clone()))
        .exec(&txn)
        .await?;
    let aliases_deleted = alias_del.rows_affected;

    // 4. Delete absorbed entities themselves
    match scope {
        Scope::Product => {
            Products::delete_many()
                .filter(products::Column::OwnerId.eq(owner_id))
                .filter(products::Column::Id.is_in(body.absorb_ids.clone()))
                .exec(&txn)
                .await?;
        }
        Scope::Merchant => {
            Merchants::delete_many()
                .filter(merchants::Column::OwnerId.eq(owner_id))
                .filter(merchants::Column::Id.is_in(body.absorb_ids.clone()))
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
