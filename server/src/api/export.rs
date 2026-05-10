/// M4-B — xlsx export
///
/// GET /api/export/:year/:month
/// Returns an .xlsx with three sheets:
///   - Transactions: every transactions row in the month, with joined entity names
///   - Settlement:   v_monthly_settlement summary (recognized / deducted / deposit)
///   - Summary:      category × actor pivot
use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use rust_xlsxwriter::{Format, FormatAlign, Workbook};
use sqlx::PgPool;
use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::ExtractUser;
use crate::error::{AppError, AppResult};

pub async fn handle_get_export(
    State(pool): State<Arc<PgPool>>,
    ExtractUser(user): ExtractUser,
    Path((year, month)): Path<(i32, i32)>,
) -> AppResult<impl IntoResponse> {
    if !(1..=12).contains(&month) {
        return Err(AppError::BadRequest(format!(
            "Invalid month {} (must be 1..=12)",
            month
        )));
    }
    let owner_id = user.sub;

    let bytes = build_workbook(&pool, owner_id, year, month).await?;

    let filename = format!("finance-{:04}-{:02}.xlsx", year, month);
    let disposition = format!("attachment; filename=\"{}\"", filename);

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        ),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&disposition).unwrap(),
    );

    Ok((StatusCode::OK, headers, bytes))
}

async fn build_workbook(
    pool: &PgPool,
    owner_id: Uuid,
    year: i32,
    month: i32,
) -> Result<Vec<u8>, AppError> {
    let mut wb = Workbook::new();

    let header_fmt = Format::new()
        .set_bold()
        .set_background_color("#E5E7EB")
        .set_align(FormatAlign::Center);
    let money_fmt = Format::new().set_num_format("#,##0");
    let date_fmt = Format::new().set_num_format("yyyy-mm-dd");

    // ── Sheet 1: Transactions ────────────────────────────────────────────────
    let txns = fetch_transactions(pool, owner_id, year, month).await?;
    {
        let sheet = wb.add_worksheet().set_name("Transactions").map_err(xlsx_err)?;
        let cols = [
            "occurred_on",
            "actor",
            "category",
            "kind",
            "merchant",
            "memo",
            "amount",
            "payment_method",
            "unit_price",
            "quantity",
            "group_id",
        ];
        for (i, c) in cols.iter().enumerate() {
            sheet
                .write_string_with_format(0, i as u16, *c, &header_fmt)
                .map_err(xlsx_err)?;
        }

        for (r, t) in txns.iter().enumerate() {
            let row = (r + 1) as u32;
            // col 0: occurred_on
            sheet
                .write_with_format(row, 0, &t.occurred_on, &date_fmt)
                .map_err(xlsx_err)?;
            // col 1: actor
            sheet
                .write_string(row, 1, t.actor.as_deref().unwrap_or(""))
                .map_err(xlsx_err)?;
            // col 2: category
            sheet
                .write_string(row, 2, t.category.as_deref().unwrap_or(""))
                .map_err(xlsx_err)?;
            // col 3: kind
            sheet
                .write_string(row, 3, t.kind.as_deref().unwrap_or(""))
                .map_err(xlsx_err)?;
            // col 4: merchant
            sheet
                .write_string(row, 4, t.merchant.as_deref().unwrap_or(""))
                .map_err(xlsx_err)?;
            // col 5: memo
            sheet
                .write_string(row, 5, t.memo.as_deref().unwrap_or(""))
                .map_err(xlsx_err)?;
            // col 6: amount (signed cash-flow value directly)
            sheet
                .write_number_with_format(row, 6, decimal_to_f64(&t.amount), &money_fmt)
                .map_err(xlsx_err)?;
            // col 7: payment_method
            sheet
                .write_string(row, 7, t.payment_method.as_deref().unwrap_or(""))
                .map_err(xlsx_err)?;
            // col 8: unit_price
            if let Some(up) = &t.unit_price {
                sheet
                    .write_number_with_format(row, 8, decimal_to_f64(up), &money_fmt)
                    .map_err(xlsx_err)?;
            }
            // col 9: quantity
            if let Some(q) = &t.quantity {
                sheet.write_number(row, 9, decimal_to_f64(q)).map_err(xlsx_err)?;
            }
            // col 10: group_id
            sheet
                .write_string(row, 10, &t.group_id.to_string())
                .map_err(xlsx_err)?;
        }

        sheet.set_column_width(0, 12.0).ok();
        sheet.set_column_width(1, 14.0).ok();
        sheet.set_column_width(2, 14.0).ok();
        sheet.set_column_width(4, 16.0).ok();
        sheet.set_column_width(5, 24.0).ok();
        sheet.set_column_width(10, 36.0).ok();
    }

    // ── Sheet 2: Settlement ──────────────────────────────────────────────────
    let s = fetch_settlement(pool, owner_id, year, month).await?;
    {
        let sheet = wb.add_worksheet().set_name("Settlement").map_err(xlsx_err)?;
        sheet
            .write_string_with_format(0, 0, "year", &header_fmt)
            .map_err(xlsx_err)?;
        sheet
            .write_string_with_format(0, 1, "month", &header_fmt)
            .map_err(xlsx_err)?;
        sheet
            .write_string_with_format(0, 2, "recognized_expense", &header_fmt)
            .map_err(xlsx_err)?;
        sheet
            .write_string_with_format(0, 3, "deducted_amount", &header_fmt)
            .map_err(xlsx_err)?;
        sheet
            .write_string_with_format(0, 4, "settlement_input", &header_fmt)
            .map_err(xlsx_err)?;
        sheet.write_number(1, 0, year as f64).map_err(xlsx_err)?;
        sheet.write_number(1, 1, month as f64).map_err(xlsx_err)?;
        sheet
            .write_number_with_format(1, 2, decimal_to_f64(&s.recognized), &money_fmt)
            .map_err(xlsx_err)?;
        sheet
            .write_number_with_format(1, 3, decimal_to_f64(&s.deducted), &money_fmt)
            .map_err(xlsx_err)?;
        sheet
            .write_number_with_format(1, 4, decimal_to_f64(&s.settlement), &money_fmt)
            .map_err(xlsx_err)?;
        for (i, w) in [8.0, 8.0, 22.0, 18.0, 20.0].iter().enumerate() {
            sheet.set_column_width(i as u16, *w).ok();
        }
    }

    // ── Sheet 3: Summary (category × actor pivot) ────────────────────────────
    let pivot = fetch_summary(pool, owner_id, year, month).await?;
    {
        let sheet = wb.add_worksheet().set_name("Summary").map_err(xlsx_err)?;
        sheet
            .write_string_with_format(0, 0, "category", &header_fmt)
            .map_err(xlsx_err)?;
        for (i, a) in pivot.actors.iter().enumerate() {
            sheet
                .write_string_with_format(0, (i + 1) as u16, a, &header_fmt)
                .map_err(xlsx_err)?;
        }
        let total_col = (pivot.actors.len() + 1) as u16;
        sheet
            .write_string_with_format(0, total_col, "합계", &header_fmt)
            .map_err(xlsx_err)?;

        for (r, (cat, by_actor)) in pivot.rows.iter().enumerate() {
            let row = (r + 1) as u32;
            sheet.write_string(row, 0, cat).map_err(xlsx_err)?;
            let mut row_total = Decimal::ZERO;
            for (i, a) in pivot.actors.iter().enumerate() {
                let v = by_actor.get(a).cloned().unwrap_or(Decimal::ZERO);
                row_total += v;
                if v != Decimal::ZERO {
                    sheet
                        .write_number_with_format(
                            row,
                            (i + 1) as u16,
                            decimal_to_f64(&v),
                            &money_fmt,
                        )
                        .map_err(xlsx_err)?;
                }
            }
            sheet
                .write_number_with_format(row, total_col, decimal_to_f64(&row_total), &money_fmt)
                .map_err(xlsx_err)?;
        }
        sheet.set_column_width(0, 18.0).ok();
    }

    wb.save_to_buffer().map_err(xlsx_err)
}

fn xlsx_err(e: rust_xlsxwriter::XlsxError) -> AppError {
    AppError::Internal(anyhow::anyhow!("xlsx error: {e}"))
}

fn decimal_to_f64(d: &Decimal) -> f64 {
    d.to_string().parse::<f64>().unwrap_or(0.0)
}

// ── Per-section data fetchers ────────────────────────────────────────────────

struct ExportTxn {
    occurred_on: NaiveDate,
    merchant: Option<String>,
    actor: Option<String>,
    category: Option<String>,
    kind: Option<String>,
    payment_method: Option<String>,
    amount: Decimal,
    unit_price: Option<Decimal>,
    quantity: Option<Decimal>,
    memo: Option<String>,
    group_id: Uuid,
}

async fn fetch_transactions(
    pool: &PgPool,
    owner_id: Uuid,
    year: i32,
    month: i32,
) -> Result<Vec<ExportTxn>, AppError> {
    let rows = sqlx::query!(
        r#"
        SELECT
            t.occurred_on  AS "occurred_on!: NaiveDate",
            m.name         AS "merchant?: String",
            a.name         AS "actor?: String",
            c.name         AS "category?: String",
            c.kind         AS "kind?: String",
            pm.name        AS "payment_method?: String",
            t.amount       AS "amount!: Decimal",
            t.unit_price   AS "unit_price?: Decimal",
            t.quantity     AS "quantity?: Decimal",
            t.memo         AS "memo?: String",
            t.group_id     AS "group_id!: Uuid"
        FROM transactions t
        LEFT JOIN merchants m        ON m.id  = t.merchant_id        AND m.owner_id  = t.owner_id
        LEFT JOIN ledger_actors a    ON a.id  = t.actor_id           AND a.owner_id  = t.owner_id
        LEFT JOIN categories c       ON c.id  = t.category_id        AND c.owner_id  = t.owner_id
        LEFT JOIN payment_methods pm ON pm.id = t.payment_method_id  AND pm.owner_id = t.owner_id
        WHERE t.owner_id = $1
          AND t.occurred_on >= make_date($2, $3, 1)
          AND t.occurred_on  < make_date($2, $3, 1) + INTERVAL '1 month'
        ORDER BY t.occurred_on, t.group_id, t.id
        "#,
        owner_id,
        year,
        month,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| ExportTxn {
            occurred_on: r.occurred_on,
            merchant: r.merchant,
            actor: r.actor,
            category: r.category,
            kind: r.kind,
            payment_method: r.payment_method,
            amount: r.amount,
            unit_price: r.unit_price,
            quantity: r.quantity,
            memo: r.memo,
            group_id: r.group_id,
        })
        .collect())
}

struct ExportSettlement {
    recognized: Decimal,
    deducted: Decimal,
    settlement: Decimal,
}

async fn fetch_settlement(
    pool: &PgPool,
    owner_id: Uuid,
    year: i32,
    month: i32,
) -> Result<ExportSettlement, AppError> {
    let row = sqlx::query!(
        r#"
        SELECT
            recognized_expense AS "recognized_expense!: Decimal",
            deducted_amount    AS "deducted_amount!: Decimal",
            settlement_input   AS "settlement_input!: Decimal"
        FROM v_monthly_settlement
        WHERE owner_id = $1 AND month = make_date($2, $3, 1)
        "#,
        owner_id,
        year,
        month,
    )
    .fetch_optional(pool)
    .await?;

    Ok(match row {
        Some(r) => ExportSettlement {
            recognized: r.recognized_expense,
            deducted: r.deducted_amount,
            settlement: r.settlement_input,
        },
        None => ExportSettlement {
            recognized: Decimal::ZERO,
            deducted: Decimal::ZERO,
            settlement: Decimal::ZERO,
        },
    })
}

struct ExportPivot {
    actors: Vec<String>,
    rows: Vec<(String, BTreeMap<String, Decimal>)>,
}

async fn fetch_summary(
    pool: &PgPool,
    owner_id: Uuid,
    year: i32,
    month: i32,
) -> Result<ExportPivot, AppError> {
    let rows = sqlx::query!(
        r#"
        SELECT
            c.name AS "category!: String",
            COALESCE(a.name, '(미지정)') AS "actor!: String",
            (-SUM(t.amount))::text AS "net_text?: String"
        FROM transactions t
        JOIN categories c         ON c.id = t.category_id AND c.owner_id = t.owner_id
        LEFT JOIN ledger_actors a ON a.id = t.actor_id    AND a.owner_id = t.owner_id
        WHERE t.owner_id = $1
          AND c.kind = 'expense'
          AND t.occurred_on >= make_date($2, $3, 1)
          AND t.occurred_on  < make_date($2, $3, 1) + INTERVAL '1 month'
        GROUP BY c.name, a.name
        ORDER BY c.name, a.name
        "#,
        owner_id,
        year,
        month,
    )
    .fetch_all(pool)
    .await?;

    let mut actors_seen: BTreeSet<String> = BTreeSet::new();
    let mut by_cat: BTreeMap<String, BTreeMap<String, Decimal>> = BTreeMap::new();
    for r in rows {
        actors_seen.insert(r.actor.clone());
        let amt = r
            .net_text
            .as_deref()
            .and_then(|s| Decimal::from_str(s).ok())
            .unwrap_or(Decimal::ZERO);
        by_cat
            .entry(r.category)
            .or_default()
            .insert(r.actor, amt);
    }

    Ok(ExportPivot {
        actors: actors_seen.into_iter().collect(),
        rows: by_cat.into_iter().collect(),
    })
}
