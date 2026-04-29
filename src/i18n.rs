pub fn get_message(key: &str, locale: &str) -> String {
    rust_i18n::t!(key, locale = locale).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_i18n_messages() {
        assert_eq!(get_message("health_ok", "en"), "Systems are operational.");
        assert_eq!(
            get_message("health_ok", "ko"),
            "시스템이 정상 작동 중입니다."
        );
    }
}
