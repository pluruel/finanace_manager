use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Serialize;
use uuid::Uuid;

/// 엑셀에서 파싱된 원시 행 (transactions_raw에 저장될 형태)
#[derive(Debug, Clone)]
pub struct RawRow {
    pub row_index: i32,
    pub group_id: Uuid,
    pub is_group_header: bool,
    pub occurred_on: Option<NaiveDate>,
    pub raw_date_serial: Option<f64>,
    pub merchant_text: Option<String>,
    pub actor_text: Option<String>,
    pub category_text: Option<String>,
    pub total_amount: Option<Decimal>,
    pub memo: Option<String>,
    pub unit_price: Option<Decimal>,
    pub quantity: Option<Decimal>,
    pub line_amount: Option<Decimal>,
    pub payment_text: Option<String>,
    pub evidence_text: Option<String>,
    pub extras: Option<serde_json::Value>,
}

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
    pub amount: Decimal,
    pub sign: i16,
    pub unit_price: Option<Decimal>,
    pub quantity: Option<Decimal>,
    pub memo: Option<String>,
}

/// 임포트 결과 응답
#[derive(Debug, Serialize)]
pub struct ImportResult {
    pub batch_id: Uuid,
    pub year: i32,
    pub month: i32,
    pub row_count: i32,
    pub transactions_inserted: i64,
    pub integrity_warnings: Vec<IntegrityWarning>,
    pub unresolved_aliases: Vec<UnresolvedAlias>,
}

/// 그룹 합계 무결성 불일치 경고
#[derive(Debug, Serialize)]
pub struct IntegrityWarning {
    pub group_id: Uuid,
    pub header_total: Decimal,
    pub lines_sum: Decimal,
}

/// 미매칭 alias (리뷰 큐)
#[derive(Debug, Serialize, Clone)]
pub struct UnresolvedAlias {
    pub scope: String,
    pub raw_text: String,
    pub norm_key: String,
}

