/// 테스트 1: 정규화 단위 테스트 (to_norm_key)
///
/// 검증 대상:
/// - 공백 trim + 다중 공백 단일화
/// - 언더스코어 → 공백 동일시
/// - NFC ↔ NFD 동일키
/// - 영문 대소문자 동일키

use finance_manager::import::normalize::to_norm_key;
use unicode_normalization::UnicodeNormalization;

#[test]
fn norm_key_space_equivalence() {
    // "이마트" vs "이 마트" vs " 이마트 " — 선행/후행 공백은 같아야 하지만
    // 중간 공백이 다르면(1개 vs 0개) 다른 키임을 확인
    assert_eq!(to_norm_key("  이마트  "), "이마트");
    // "이마트"와 "이 마트"는 중간에 공백이 있어 다른 키여야 한다
    // (도메인상 같은 구매처가 아님)
    assert_ne!(to_norm_key("이마트"), to_norm_key("이 마트"));
}

#[test]
fn norm_key_underscore_as_space() {
    // 언더스코어는 공백과 동일하게 취급
    assert_eq!(to_norm_key("외식_점심"), "외식 점심");
    assert_eq!(to_norm_key("외식 점심"), "외식 점심");
    assert_eq!(to_norm_key("외식_점심"), to_norm_key("외식 점심"));
}

#[test]
fn norm_key_multiple_underscore_and_spaces() {
    // 내부 다중 공백/언더스코어 → 단일 공백
    assert_eq!(to_norm_key("외식__점심"), "외식 점심");
    assert_eq!(to_norm_key("외식  점심"), "외식 점심");
    assert_eq!(to_norm_key("외식 _점심"), "외식 점심");
}

#[test]
fn norm_key_english_case_insensitive() {
    // 영문 대소문자 동일
    assert_eq!(to_norm_key("ABC"), "abc");
    assert_eq!(to_norm_key("Hello World"), "hello world");
    assert_eq!(to_norm_key("ABC"), to_norm_key("abc"));
    assert_eq!(to_norm_key("외식_A"), to_norm_key("외식_a"));
}

#[test]
fn norm_key_nfc_nfd_equivalence() {
    // NFC ↔ NFD 정규화: 같은 한글 문자를 NFC/NFD로 표현해도 같은 키
    let nfc_str = "조닌끼안티"; // NFC
    // NFD로 분해: 자음+모음 분리
    let nfd_str: String = nfc_str.nfd().collect();

    // nfd_str이 nfc_str과 다른 바이트라면(한글이면 보통 다름) 테스트 의미 있음
    // 동일 바이트면 NFC/NFD 변환이 없는 문자
    let norm_nfc = to_norm_key(nfc_str);
    let norm_nfd = to_norm_key(&nfd_str);
    assert_eq!(norm_nfc, norm_nfd,
        "NFC({:?}) vs NFD({:?}) should produce same norm_key",
        nfc_str, nfd_str
    );
}

#[test]
fn norm_key_trim_and_collapse() {
    // 선행/후행 공백 제거 + 내부 다중 공백 단일화
    assert_eq!(to_norm_key("  hello   world  "), "hello world");
    assert_eq!(to_norm_key("\t외식\t점심\t"), "외식 점심");
}

#[test]
fn norm_key_empty_and_whitespace() {
    // 빈 문자열, 공백 전용 문자열
    assert_eq!(to_norm_key(""), "");
    assert_eq!(to_norm_key("   "), "");
}

#[test]
fn norm_key_category_equivalence() {
    // 카테고리 "외식_점심" vs "외식 점심" → 같은 키 (임포트 규칙)
    let k1 = to_norm_key("외식_점심");
    let k2 = to_norm_key("외식 점심");
    assert_eq!(k1, k2, "카테고리 언더스코어/공백 동일시 실패");
}

#[test]
fn norm_key_merchant_deduplication() {
    // 구매처 "이마트" vs " 이마트 " → 같은 키 (공백만 다름)
    assert_eq!(to_norm_key("이마트"), to_norm_key("  이마트  "));
    assert_eq!(to_norm_key("이마트"), to_norm_key("이마트"));
}
