use unicode_normalization::UnicodeNormalization;

/// norm_key 생성 함수 (PLAN §3)
/// 1. Unicode NFC 정규화
/// 2. 양끝 trim, 내부 다중 공백 → 단일 공백
/// 3. '_' → ' ' (언더스코어와 공백 동일시)
/// 4. 한글은 그대로, 영문은 to_lowercase
pub fn to_norm_key(s: &str) -> String {
    // 1. NFC 정규화
    let nfc: String = s.nfc().collect();

    // 2. _ → 공백
    let replaced = nfc.replace('_', " ");

    // 3. 영문 소문자
    let lowered = replaced.to_lowercase();

    // 4. trim + 내부 다중 공백 → 단일 공백
    let trimmed = lowered.split_whitespace().collect::<Vec<_>>().join(" ");

    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_norm_key() {
        assert_eq!(to_norm_key("외식_점심"), "외식 점심");
        assert_eq!(to_norm_key("이 마트"), "이 마트");
        assert_eq!(to_norm_key("  이마트  "), "이마트");
        assert_eq!(to_norm_key("ABC"), "abc");
        assert_eq!(to_norm_key("외식_점심_A"), "외식 점심 a");
    }
}
