use serde_json::Value;
use std::collections::HashMap;

/// Choose the first supported OS language when creating a new configuration.
/// Saved configurations keep their explicit language selection.
pub fn default_language() -> String {
    language_from_locales(sys_locale::get_locales()).to_string()
}

fn language_from_locales(locales: impl IntoIterator<Item = impl AsRef<str>>) -> &'static str {
    locales
        .into_iter()
        .find_map(|locale| supported_language(locale.as_ref()))
        .unwrap_or("en")
}

/// Normalize OS locale tags to the identifiers shared by the UI and tray menu.
fn supported_language(locale: &str) -> Option<&'static str> {
    let locale = locale
        .split(['.', '@'])
        .next()?
        .replace('_', "-")
        .to_ascii_lowercase();
    let mut subtags = locale.split('-');
    match subtags.next()? {
        "zh" => Some(match subtags.next() {
            // An explicit script takes precedence over the region (zh-Hans-TW).
            Some("hant" | "tw" | "hk" | "mo") => "zh-TW",
            _ => "zh",
        }),
        "en" => Some("en"),
        "ja" => Some("ja"),
        "tr" => Some("tr"),
        "vi" => Some("vi"),
        "pt" => Some("pt"),
        "ru" => Some("ru"),
        "ko" => Some("ko"),
        "ar" => Some("ar"),
        "es" => Some("es"),
        // The existing Malay translation uses the application's legacy "my" key.
        "ms" => Some("my"),
        _ => None,
    }
}

/// Tray text structure
#[derive(Debug, Clone)]
pub struct TrayTexts {
    pub current: String,
    pub quota: String,
    pub switch_next: String,
    pub refresh_current: String,
    pub show_window: String,
    pub quit: String,
    pub no_account: String,
    pub unknown_quota: String,
    pub forbidden: String,
    pub lightweight_mode: String,
}

/// Load translations from JSON
fn load_translations(lang: &str) -> HashMap<String, String> {
    // Map every language the frontend supports (see src/i18n.ts / navbar/constants.ts)
    // so the tray menu follows the in-app language switch. Unknown codes fall back to
    // English, matching the frontend's fallbackLng.
    let json_content = match lang {
        "zh" | "zh-CN" | "zh-Hans" => include_str!("../../../src/locales/zh.json"),
        "zh-TW" | "zh-Hant" => include_str!("../../../src/locales/zh-TW.json"),
        "ja" | "ja-JP" => include_str!("../../../src/locales/ja.json"),
        "tr" | "tr-TR" => include_str!("../../../src/locales/tr.json"),
        "vi" | "vi-VN" => include_str!("../../../src/locales/vi.json"),
        "pt" | "pt-BR" | "pt-PT" => include_str!("../../../src/locales/pt.json"),
        "ru" | "ru-RU" => include_str!("../../../src/locales/ru.json"),
        "ko" | "ko-KR" => include_str!("../../../src/locales/ko.json"),
        "ar" | "ar-SA" => include_str!("../../../src/locales/ar.json"),
        "es" | "es-ES" | "es-MX" => include_str!("../../../src/locales/es.json"),
        "my" | "ms" | "ms-MY" => include_str!("../../../src/locales/my.json"),
        "en" | "en-US" => include_str!("../../../src/locales/en.json"),
        _ => include_str!("../../../src/locales/en.json"),
    };

    let v: Value = serde_json::from_str(json_content).unwrap_or_else(|_| serde_json::json!({}));

    let mut map = HashMap::new();

    if let Some(tray) = v.get("tray").and_then(|t| t.as_object()) {
        for (key, value) in tray {
            if let Some(s) = value.as_str() {
                map.insert(key.clone(), s.to_string());
            }
        }
    }

    map
}

/// Get tray texts (based on language)
pub fn get_tray_texts(lang: &str) -> TrayTexts {
    let t = load_translations(lang);

    TrayTexts {
        current: t
            .get("current")
            .cloned()
            .unwrap_or_else(|| "Current".to_string()),
        quota: t
            .get("quota")
            .cloned()
            .unwrap_or_else(|| "Quota".to_string()),
        switch_next: t
            .get("switch_next")
            .cloned()
            .unwrap_or_else(|| "Switch to Next Account".to_string()),
        refresh_current: t
            .get("refresh_current")
            .cloned()
            .unwrap_or_else(|| "Refresh Current Quota".to_string()),
        show_window: t
            .get("show_window")
            .cloned()
            .unwrap_or_else(|| "Show Main Window".to_string()),
        quit: t
            .get("quit")
            .cloned()
            .unwrap_or_else(|| "Quit Application".to_string()),
        no_account: t
            .get("no_account")
            .cloned()
            .unwrap_or_else(|| "No Account".to_string()),
        unknown_quota: t
            .get("unknown_quota")
            .cloned()
            .unwrap_or_else(|| "Unknown".to_string()),
        forbidden: t
            .get("forbidden")
            .cloned()
            .unwrap_or_else(|| "Account Forbidden".to_string()),
        lightweight_mode: t
            .get("lightweight_mode")
            .cloned()
            .unwrap_or_else(|| "Lightweight Mode".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{get_tray_texts, language_from_locales};

    #[test]
    fn detects_supported_languages_from_os_locale_tags() {
        for (locale, expected) in [
            ("en-US", "en"),
            ("en-GB", "en"),
            ("ru-RU", "ru"),
            ("ja-JP", "ja"),
            ("tr-TR", "tr"),
            ("vi-VN", "vi"),
            ("pt-BR", "pt"),
            ("pt-PT", "pt"),
            ("ko-KR", "ko"),
            ("ar-SA", "ar"),
            ("es-MX", "es"),
            ("ms-MY", "my"),
            ("RU_ru.UTF-8", "ru"),
            ("es_ES@euro", "es"),
        ] {
            assert_eq!(language_from_locales([locale]), expected, "{locale}");
        }
    }

    #[test]
    fn distinguishes_chinese_scripts_and_regions() {
        for (locale, expected) in [
            ("zh", "zh"),
            ("zh-CN", "zh"),
            ("zh-SG", "zh"),
            ("zh-Hans", "zh"),
            ("zh-Hans-TW", "zh"),
            ("zh-TW", "zh-TW"),
            ("zh-HK", "zh-TW"),
            ("zh-MO", "zh-TW"),
            ("zh-Hant", "zh-TW"),
            ("zh-Hant-CN", "zh-TW"),
        ] {
            assert_eq!(language_from_locales([locale]), expected, "{locale}");
        }
    }

    #[test]
    fn honors_preference_order_and_skips_unsupported_languages() {
        assert_eq!(language_from_locales(["de-DE", "ru-RU", "en-US"]), "ru");
        assert_eq!(language_from_locales(["en-GB", "zh-CN"]), "en");
        assert_eq!(language_from_locales(["zh-TW", "en-US"]), "zh-TW");
    }

    #[test]
    fn falls_back_to_english_without_a_supported_locale() {
        assert_eq!(language_from_locales(Vec::<String>::new()), "en");
        for locale in ["", "C", "POSIX", "C.UTF-8", "de-DE", "my-MM"] {
            assert_eq!(language_from_locales([locale]), "en", "{locale}");
        }
    }

    #[test]
    fn tray_uses_the_detected_language() {
        let language = language_from_locales(["ru-RU"]);
        let texts = get_tray_texts(language);
        let russian: serde_json::Value =
            serde_json::from_str(include_str!("../../../src/locales/ru.json")).unwrap();
        assert_eq!(texts.quit, russian["tray"]["quit"].as_str().unwrap());
    }
}
