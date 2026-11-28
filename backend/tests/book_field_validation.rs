use socrates_chat_backend::{book_store::BookData, books::normalize_book};
#[test]
fn book_language_limits_and_legacy_labels() {
    for (raw, expected) in [
        ("中文", "zh"),
        ("英文", "en"),
        ("西文", "es"),
        ("zh-Hant-TW", "zh-Hant-TW"),
        ("eng", "en"),
        ("fr", "fr"),
    ] {
        let book = normalize_book(BookData {
            title: "Synthetic".into(),
            language: Some(raw.into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(book.language.as_deref(), Some(expected));
    }
    for raw in ["x".repeat(36), "not a language".into(), "en<script>".into()] {
        assert_eq!(
            normalize_book(BookData {
                title: "Synthetic".into(),
                language: Some(raw),
                ..Default::default()
            })
            .unwrap_err()
            .code,
            "invalid_book_language"
        );
    }
    assert!(
        normalize_book(BookData {
            title: "Synthetic".into(),
            language: Some("x".repeat(35)),
            ..Default::default()
        })
        .is_ok()
    );
}
#[test]
fn book_publication_year_accepts_only_optional_four_digit_year() {
    for raw in ["0001", "2026", "9999", " 2020 ", ""] {
        assert!(
            normalize_book(BookData {
                title: "Synthetic".into(),
                publication_year: Some(raw.into()),
                ..Default::default()
            })
            .is_ok()
        );
    }
    for raw in ["0000", "123", "10000", "2020-01-01", "約2020年", "２０２０"] {
        assert_eq!(
            normalize_book(BookData {
                title: "Synthetic".into(),
                publication_year: Some(raw.into()),
                ..Default::default()
            })
            .unwrap_err()
            .code,
            "invalid_book_publication_year"
        );
    }
}
