//! The supported regional catalogue. Every choice is a region-bearing code so
//! number, currency and later date formatting keep the region; the language
//! part is carried separately for a translation adapter. These are formatting
//! choices, not a claim that product copy is translated into them.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Entry {
    pub code: &'static str,
    pub label: &'static str,
    pub language: &'static str,
}

const fn entry(code: &'static str, label: &'static str, language: &'static str) -> Entry {
    Entry {
        code,
        label,
        language,
    }
}

/// Sorted by code so lookups can binary search and selectors list stably.
pub static SUPPORTED: &[Entry] = &[
    entry("ar-SA", "Arabic (Saudi Arabia)", "ar"),
    entry("de-CH", "German (Switzerland)", "de"),
    entry("de-DE", "German (Germany)", "de"),
    entry("en-AU", "English (Australia)", "en"),
    entry("en-CA", "English (Canada)", "en"),
    entry("en-GB", "English (United Kingdom)", "en"),
    entry("en-MY", "English (Malaysia)", "en"),
    entry("en-SG", "English (Singapore)", "en"),
    entry("en-US", "English (United States)", "en"),
    entry("es-ES", "Spanish (Spain)", "es"),
    entry("fr-CA", "French (Canada)", "fr"),
    entry("fr-FR", "French (France)", "fr"),
    entry("hi-IN", "Hindi (India)", "hi"),
    entry("id-ID", "Indonesian (Indonesia)", "id"),
    entry("it-IT", "Italian (Italy)", "it"),
    entry("ja-JP", "Japanese (Japan)", "ja"),
    entry("ko-KR", "Korean (South Korea)", "ko"),
    entry("ms-MY", "Malay (Malaysia)", "ms"),
    entry("nl-NL", "Dutch (Netherlands)", "nl"),
    entry("pl-PL", "Polish (Poland)", "pl"),
    entry("pt-BR", "Portuguese (Brazil)", "pt"),
    entry("ru-RU", "Russian (Russia)", "ru"),
    entry("th-TH", "Thai (Thailand)", "th"),
    entry("tr-TR", "Turkish (Turkey)", "tr"),
    entry("vi-VN", "Vietnamese (Vietnam)", "vi"),
    entry("zh-CN", "Chinese (China)", "zh"),
    entry("zh-HK", "Chinese (Hong Kong)", "zh"),
    entry("zh-TW", "Chinese (Taiwan)", "zh"),
];

/// The regional code a bare language resolves to.
pub static LANGUAGE_DEFAULTS: &[(&str, &str)] = &[
    ("ar", "ar-SA"),
    ("de", "de-DE"),
    ("en", "en-MY"),
    ("es", "es-ES"),
    ("fr", "fr-FR"),
    ("hi", "hi-IN"),
    ("id", "id-ID"),
    ("it", "it-IT"),
    ("ja", "ja-JP"),
    ("ko", "ko-KR"),
    ("ms", "ms-MY"),
    ("nl", "nl-NL"),
    ("pl", "pl-PL"),
    ("pt", "pt-BR"),
    ("ru", "ru-RU"),
    ("th", "th-TH"),
    ("tr", "tr-TR"),
    ("vi", "vi-VN"),
    ("zh", "zh-CN"),
];

/// Countries whose address-derived locale is fixed regardless of language lists.
pub static COUNTRY_OVERRIDES: &[(&str, &str)] = &[
    ("AU", "en-AU"),
    ("BR", "pt-BR"),
    ("CA", "en-CA"),
    ("CH", "de-CH"),
    ("CN", "zh-CN"),
    ("DE", "de-DE"),
    ("ES", "es-ES"),
    ("FR", "fr-FR"),
    ("GB", "en-GB"),
    ("HK", "zh-HK"),
    ("ID", "id-ID"),
    ("IN", "hi-IN"),
    ("IT", "it-IT"),
    ("JP", "ja-JP"),
    ("KR", "ko-KR"),
    ("MY", "en-MY"),
    ("NL", "nl-NL"),
    ("PL", "pl-PL"),
    ("RU", "ru-RU"),
    ("SA", "ar-SA"),
    ("SG", "en-SG"),
    ("TH", "th-TH"),
    ("TR", "tr-TR"),
    ("TW", "zh-TW"),
    ("US", "en-US"),
    ("VN", "vi-VN"),
];

pub fn find(code: &str) -> Option<&'static Entry> {
    SUPPORTED
        .binary_search_by(|entry| entry.code.cmp(code))
        .ok()
        .map(|index| &SUPPORTED[index])
}

pub fn language_default(language: &str) -> Option<&'static str> {
    LANGUAGE_DEFAULTS
        .iter()
        .find(|(code, _)| *code == language)
        .map(|(_, locale)| *locale)
}

pub fn country_override(country: &str) -> Option<&'static str> {
    COUNTRY_OVERRIDES
        .iter()
        .find(|(code, _)| *code == country)
        .map(|(_, locale)| *locale)
}
