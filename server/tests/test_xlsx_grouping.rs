/// 테스트 2: 엑셀 그룹 검출 테스트
///
/// 골든 케이스: "2026년 02월.xlsx" (2월 시트)
/// 검증:
/// - multi-line 그룹 7개
/// - 풍림아이원 그룹 (row 127) = 헤더+자식 17개 (총 17행)
/// - Decimal 정밀도 보존 (그룹 합계 무결성)
/// - 총 유효 행 177개 → transactions 예상 177개

use finance_manager::import::xlsx::parse_xlsx;
use rust_decimal::Decimal;
use std::collections::HashMap;

fn load_golden_xlsx() -> Vec<u8> {
    // 테스트 fixtures 경로: CARGO_MANIFEST_DIR 기준
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/2026년_02월.xlsx"
    );
    std::fs::read(path).expect("Failed to read golden xlsx fixture")
}

#[test]
fn parse_golden_xlsx_row_count() {
    let bytes = load_golden_xlsx();
    let rows = parse_xlsx(&bytes, "2월").expect("parse_xlsx failed");
    // 날짜 있는 유효 행 177개
    assert_eq!(
        rows.len(),
        177,
        "Expected 177 raw rows from golden xlsx, got {}",
        rows.len()
    );
}

#[test]
fn parse_golden_xlsx_multi_line_groups() {
    let bytes = load_golden_xlsx();
    let rows = parse_xlsx(&bytes, "2월").expect("parse_xlsx failed");

    // group_id별 그룹핑
    let mut group_map: HashMap<uuid::Uuid, Vec<&finance_manager::domain::RawRow>> =
        HashMap::new();
    for row in &rows {
        group_map.entry(row.group_id).or_default().push(row);
    }

    let multi_line: Vec<_> = group_map.values().filter(|g| g.len() > 1).collect();
    assert_eq!(
        multi_line.len(),
        7,
        "Expected 7 multi-line groups, got {}. Groups: {:?}",
        multi_line.len(),
        multi_line.iter().map(|g| (g[0].merchant_text.as_deref(), g.len())).collect::<Vec<_>>()
    );
}

#[test]
fn parse_golden_xlsx_punglim_group_size() {
    let bytes = load_golden_xlsx();
    let rows = parse_xlsx(&bytes, "2월").expect("parse_xlsx failed");

    // 풍림아이원 헤더가 row_index=127
    let punglim_header = rows.iter().find(|r| {
        r.row_index == 127 && r.merchant_text.as_deref() == Some("풍림아이원")
    });

    assert!(
        punglim_header.is_some(),
        "풍림아이원 헤더 (row_index=127) 를 찾지 못했다"
    );

    let header = punglim_header.unwrap();
    let group_id = header.group_id;

    // 같은 group_id를 가진 행들 카운트
    let group_rows: Vec<_> = rows.iter().filter(|r| r.group_id == group_id).collect();

    assert_eq!(
        group_rows.len(),
        17,
        "풍림아이원 그룹은 헤더+자식 합쳐 17행이어야 한다, got {}",
        group_rows.len()
    );
}

#[test]
fn parse_golden_xlsx_punglim_integrity() {
    let bytes = load_golden_xlsx();
    let rows = parse_xlsx(&bytes, "2월").expect("parse_xlsx failed");

    // 풍림아이원 헤더 찾기
    let header = rows
        .iter()
        .find(|r| r.row_index == 127)
        .expect("풍림아이원 row_index=127 없음");

    let group_id = header.group_id;
    let header_total = header.total_amount.expect("헤더 total_amount 없음");

    // group의 모든 line_amount 합산 (헤더 포함)
    let group_rows: Vec<_> = rows.iter().filter(|r| r.group_id == group_id).collect();
    let lines_sum: Decimal = group_rows
        .iter()
        .filter_map(|r| r.line_amount)
        .fold(Decimal::ZERO, |acc, x| acc + x);

    assert_eq!(
        header_total, lines_sum,
        "풍림아이원 그룹 합계 무결성 실패: header_total={}, lines_sum={}",
        header_total, lines_sum
    );
}

#[test]
fn parse_golden_xlsx_header_is_group_header_flag() {
    let bytes = load_golden_xlsx();
    let rows = parse_xlsx(&bytes, "2월").expect("parse_xlsx failed");

    // is_group_header 플래그: total_amount 있는 행은 true여야 함
    for row in &rows {
        if row.total_amount.is_some() {
            assert!(
                row.is_group_header,
                "row_index={}: total_amount 있는데 is_group_header=false",
                row.row_index
            );
        }
    }
}

#[test]
fn parse_golden_xlsx_decimal_precision() {
    let bytes = load_golden_xlsx();
    let rows = parse_xlsx(&bytes, "2월").expect("parse_xlsx failed");

    // 소수점 2자리 초과 금액이 없는지 확인 (line_amount 기준)
    for row in &rows {
        if let Some(amt) = row.line_amount {
            let scale = amt.scale();
            assert!(
                scale <= 2,
                "row_index={}: line_amount={} 소수점 {}자리 초과 (최대 2자리 허용)",
                row.row_index,
                amt,
                scale
            );
        }
        // unit_price는 4자리까지 허용
        if let Some(price) = row.unit_price {
            let scale = price.scale();
            assert!(
                scale <= 4,
                "row_index={}: unit_price={} 소수점 {}자리 초과 (최대 4자리 허용)",
                row.row_index,
                price,
                scale
            );
        }
    }
}

#[test]
fn parse_golden_xlsx_all_groups_integrity() {
    let bytes = load_golden_xlsx();
    let rows = parse_xlsx(&bytes, "2월").expect("parse_xlsx failed");

    // 모든 multi-line 그룹에 대해 header_total == children line_amount 합계 검증
    let mut group_map: HashMap<uuid::Uuid, Vec<&finance_manager::domain::RawRow>> =
        HashMap::new();
    for row in &rows {
        group_map.entry(row.group_id).or_default().push(row);
    }

    let mut violations = 0;
    for (group_id, group_rows) in &group_map {
        if group_rows.len() <= 1 {
            continue; // single-line은 건너뜀
        }
        // 헤더 찾기
        let header = group_rows.iter().find(|r| r.is_group_header);
        if let Some(header) = header {
            if let Some(header_total) = header.total_amount {
                let lines_sum: Decimal = group_rows
                    .iter()
                    .filter_map(|r| r.line_amount)
                    .fold(Decimal::ZERO, |acc, x| acc + x);

                if header_total != lines_sum {
                    eprintln!(
                        "그룹 무결성 위반: group_id={}, merchant={:?}, header_total={}, lines_sum={}",
                        group_id,
                        header.merchant_text,
                        header_total,
                        lines_sum
                    );
                    violations += 1;
                }
            }
        }
    }

    assert_eq!(
        violations, 0,
        "그룹 합계 무결성 위반 {}건 발생",
        violations
    );
}
