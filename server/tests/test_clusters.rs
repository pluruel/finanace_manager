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
    assert!(clusters.iter().all(|c| c["members"].as_array().unwrap().len() >= 2));
}

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

use serde_json::json;
use finance_manager::entity::{aliases, prelude::Aliases, transactions, prelude::Transactions};
use sea_orm::{ColumnTrait, QueryFilter, PaginatorTrait};

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
        "absorb_ids": absorb_ids.clone(),
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
