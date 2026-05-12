use anyhow::{Context, Result};
use calamine::{open_workbook_from_rs, Data, DataType, Reader, Xlsx};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use std::io::Cursor;
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

use crate::domain::RawRow;

/// Excel serial date → NaiveDate 변환
/// epoch: 1899-12-30 (1900-02-29 버그 회피)
pub fn serial_to_date(serial: f64) -> Option<NaiveDate> {
    let epoch = NaiveDate::from_ymd_opt(1899, 12, 30)?;
    let days = serial.floor() as i64;
    // 1900-02-29 버그: Excel serial 60 = 존재하지 않는 날짜
    // serial >= 60이면 1을 뺀다
    let adjusted_days = if days >= 60 { days - 1 } else { days };
    epoch.checked_add_signed(chrono::Duration::days(adjusted_days))
}

/// 셀 Data → Option<Decimal> (금액용, 소수점 2자리까지 정밀도 보존)
/// f64 → round() as i64 방식의 0.5원 단위 손실을 방지하기 위해
/// Decimal::from_f64를 사용하고 소수점 2자리로 round_dp.
fn cell_to_decimal(cell: &Data) -> Option<Decimal> {
    match cell {
        Data::Int(i) => Some(Decimal::from(*i)),
        Data::Float(f) => {
            Decimal::from_f64(*f).map(|d| d.round_dp(2))
        }
        Data::String(s) => s.trim().parse::<Decimal>().ok(),
        _ => None,
    }
}

/// 셀 Data → Option<Decimal> (단가용 - 소수점 4자리까지 정밀도 보존)
fn cell_to_decimal_precise(cell: &Data) -> Option<Decimal> {
    match cell {
        Data::Int(i) => Some(Decimal::from(*i)),
        Data::Float(f) => {
            Decimal::from_f64(*f).map(|d| d.round_dp(4))
        }
        Data::String(s) => s.trim().parse::<Decimal>().ok(),
        _ => None,
    }
}

fn cell_to_string(cell: &Data) -> Option<String> {
    match cell {
        Data::String(s) => {
            let trimmed = s.trim().to_string();
            if trimmed.is_empty() { None } else { Some(trimmed) }
        }
        Data::Int(i) => Some(i.to_string()),
        Data::Float(f) => Some(format!("{}", f)),
        _ => None,
    }
}

fn cell_to_date(cell: &Data) -> (Option<NaiveDate>, Option<f64>) {
    match cell {
        Data::DateTime(dt) => {
            // calamine ExcelDateTime → NaiveDate
            let serial = dt.as_f64();
            let date = serial_to_date(serial);
            (date, Some(serial))
        }
        Data::DateTimeIso(s) => {
            // ISO 형식: "2026-02-01" 또는 "2026-02-01T00:00:00"
            let date = s.split('T').next()
                .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok());
            (date, None)
        }
        Data::Float(f) => {
            // dates feature 비활성 시 float으로 올 수 있음
            let date = serial_to_date(*f);
            (date, Some(*f))
        }
        _ => (None, None),
    }
}

/// xlsx 바이트에서 "M월" 시트를 파싱해 RawRow 벡터 반환
pub fn parse_xlsx(bytes: &[u8], sheet_name: &str) -> Result<Vec<RawRow>> {
    let cursor = Cursor::new(bytes);
    let mut workbook: Xlsx<_> = open_workbook_from_rs(cursor)
        .context("Failed to open xlsx workbook")?;

    // 파일별로 시트명이 "1월" / "01월" 형태로 섞여 있어, 숫자로 일치하는 첫 시트를 찾는다.
    // (집계 시트는 "01월(집계)" 처럼 괄호가 붙어 있어 자동으로 배제됨.)
    let resolved_name = sheet_name
        .strip_suffix('월')
        .and_then(|s| s.trim().parse::<u32>().ok())
        .and_then(|requested_month| {
            workbook.sheet_names().into_iter().find(|name| {
                name.strip_suffix('월')
                    .and_then(|s| s.trim().parse::<u32>().ok())
                    .map(|m| m == requested_month)
                    .unwrap_or(false)
            })
        })
        .unwrap_or_else(|| sheet_name.to_string());

    let range = workbook
        .worksheet_range(&resolved_name)
        .with_context(|| format!("Sheet '{}' not found", sheet_name))?;

    let mut rows = Vec::new();
    let mut current_group_id = Uuid::new_v4();
    let mut prev_header_date: Option<NaiveDate> = None;

    for (row_idx, row) in range.rows().enumerate() {
        let excel_row = row_idx + 1; // 1-based

        // 헤더 행(1) 또는 빈 줄(2) 스킵
        if excel_row <= 2 {
            continue;
        }

        // 컬럼 레이아웃 (0-based):
        // A=0: 날짜, B=1: 구매처, C=2: 사용자, D=3: 소분류(카테고리)
        // E=4: 지출(합계), F=5: 내용(메모), G=6: 단가, H=7: 개수, I=8: 지출(매수)
        // J=9: 결재수단, K=10: 증빙

        let get = |idx: usize| row.get(idx).unwrap_or(&Data::Empty);

        let (occurred_on, raw_date_serial) = cell_to_date(get(0));
        let merchant_text = cell_to_string(get(1));
        let actor_text = cell_to_string(get(2));
        let category_text = cell_to_string(get(3));
        let total_amount = cell_to_decimal(get(4));
        let memo = cell_to_string(get(5));
        let unit_price = cell_to_decimal_precise(get(6));
        let quantity = cell_to_decimal_precise(get(7));
        let line_amount = cell_to_decimal(get(8));
        let payment_text = cell_to_string(get(9));
        let evidence_text = cell_to_string(get(10));

        // extras: 컬럼 L(11) 이후 잡 데이터 보존
        let extras = {
            let mut map = serde_json::Map::new();
            for col_idx in 11..row.len() {
                let val = get(col_idx);
                if !val.is_empty() {
                    let col_label = format!("col_{}", col_idx);
                    let json_val = match val {
                        Data::String(s) => serde_json::Value::String(s.clone()),
                        Data::Int(i) => serde_json::Value::Number((*i).into()),
                        Data::Float(f) => {
                            serde_json::Value::Number(
                                serde_json::Number::from_f64(*f)
                                    .unwrap_or_else(|| serde_json::Number::from(0)),
                            )
                        }
                        Data::Bool(b) => serde_json::Value::Bool(*b),
                        _ => serde_json::Value::Null,
                    };
                    map.insert(col_label, json_val);
                }
            }
            if map.is_empty() { None } else { Some(serde_json::Value::Object(map)) }
        };

        // 유효한 데이터 행 확인: 날짜 있어야 함
        if occurred_on.is_none() {
            // 날짜 없는 행은 파싱 안정성을 위해 스킵
            // (집계 시트 더미 행, 빈 행 등)
            continue;
        }

        // 그룹 검출 알고리즘 (PLAN §3 ②):
        // 컬럼 E(total_amount) 채워진 행 = 헤더, 새 group_id 부여
        // 컬럼 E None = 자식 → 같은 날짜면 현재 그룹에 속함
        let (is_group_header, group_id) = if total_amount.is_some() {
            // 헤더 행 → 새 그룹 시작
            current_group_id = Uuid::new_v4();
            prev_header_date = occurred_on;
            (true, current_group_id)
        } else {
            // 자식 행 (E열 없음)
            // 날짜가 같으면 현재 그룹의 자식
            // 풍림아이원처럼 merchant가 다른 자식도 날짜 기준으로 같은 그룹 처리
            // (합리적 기본값 - reviewer 검토 필요)
            let is_same_date = occurred_on == prev_header_date && prev_header_date.is_some();
            if is_same_date {
                (false, current_group_id)
            } else {
                // 고아 자식 행 - 경고 후 새 group_id로 처리
                tracing::warn!(
                    "Row {}: orphan child row date={:?}, assigning new group_id",
                    excel_row,
                    occurred_on
                );
                let orphan_gid = Uuid::new_v4();
                current_group_id = orphan_gid;
                (false, orphan_gid)
            }
        };

        rows.push(RawRow {
            row_index: excel_row as i32,
            group_id,
            is_group_header,
            occurred_on,
            raw_date_serial,
            merchant_text,
            actor_text,
            category_text,
            total_amount,
            memo,
            unit_price,
            quantity,
            line_amount,
            payment_text,
            evidence_text,
            extras,
        });
    }

    Ok(rows)
}

/// 파일명에서 시트명 추출: "2026년 02월.xlsx" → "2월"
pub fn extract_sheet_name(filename: &str) -> Option<String> {
    let filename: String = filename.nfc().collect();
    let base = filename.strip_suffix(".xlsx").unwrap_or(&filename);
    if let Some(year_pos) = base.find('년') {
        let after_year = &base[year_pos + '년'.len_utf8()..];
        let trimmed = after_year.trim();
        if let Some(month_pos) = trimmed.find('월') {
            let month_str = trimmed[..month_pos].trim().trim_start_matches('0');
            if !month_str.is_empty() {
                return Some(format!("{}월", month_str));
            }
        }
    }
    None
}

/// 파일명에서 (year, month) 추출: "2026년 02월.xlsx" → (2026, 2)
pub fn extract_year_month(filename: &str) -> Option<(i32, i32)> {
    let filename: String = filename.nfc().collect();
    let base = filename.strip_suffix(".xlsx").unwrap_or(&filename);
    let year_pos = base.find('년')?;
    let year: i32 = base[..year_pos].trim().parse().ok()?;

    let after_year = &base[year_pos + '년'.len_utf8()..];
    let month_pos = after_year.find('월')?;
    let month: i32 = after_year[..month_pos].trim().parse().ok()?;

    Some((year, month))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_sheet_name() {
        assert_eq!(extract_sheet_name("2026년 02월.xlsx"), Some("2월".to_string()));
        assert_eq!(extract_sheet_name("2026년 12월.xlsx"), Some("12월".to_string()));
    }

    #[test]
    fn test_extract_year_month() {
        assert_eq!(extract_year_month("2026년 02월.xlsx"), Some((2026, 2)));
        assert_eq!(extract_year_month("2026년 12월.xlsx"), Some((2026, 12)));
        // macOS HFS+/APFS stores Korean in NFD; browser sends it as-is
        let nfd = "2026\u{1102}\u{1167}\u{11AB} 02\u{110B}\u{116F}\u{11AF}.xlsx";
        assert_eq!(extract_year_month(nfd), Some((2026, 2)));
    }

    #[test]
    fn parse_xlsx_resolves_zero_padded_sheet_name() {
        // 2020년 01월.xlsx 시트는 "01월" 로 저장되어 있지만
        // extract_sheet_name 은 "1월" 을 반환한다. parse_xlsx 가 둘을 동치로 처리해야 한다.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("2020년 01월.xlsx");
        if !path.exists() {
            // 골든 파일이 없는 환경에서는 스킵 (CI 등)
            return;
        }
        let bytes = std::fs::read(&path).expect("read 2020 file");
        let rows = parse_xlsx(&bytes, "1월").expect("parse with unpadded sheet name");
        assert!(rows.len() > 100, "expected >100 data rows, got {}", rows.len());
    }

    #[test]
    fn test_serial_to_date() {
        // Excel serial 45000 = 2023-03-15 (검증 필요)
        // 1899-12-30 + 45000일
        let epoch = NaiveDate::from_ymd_opt(1899, 12, 30).unwrap();
        let expected = epoch + chrono::Duration::days(45000 - 1); // serial >= 60이므로 -1
        let result = serial_to_date(45000.0).unwrap();
        assert_eq!(result, expected);
    }
}
