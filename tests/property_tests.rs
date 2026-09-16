use docdownloader::storage::sanitize::{safe_output_path, sanitize_filename};
use proptest::prelude::*;
use std::path::Path;

proptest! {
    #[test]
    fn prop_sanitized_filename_never_contains_forbidden_chars(raw in "\\PC*") {
        let clean = sanitize_filename(&raw);
        let forbidden = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
        for c in forbidden {
            prop_assert!(!clean.contains(c), "Cleaned filename '{clean}' contains forbidden '{c}'");
        }
    }

    #[test]
    fn prop_sanitized_filename_never_empty(raw in "\\PC*") {
        let clean = sanitize_filename(&raw);
        prop_assert!(!clean.trim().is_empty(), "Cleaned filename must never be empty");
    }

    #[test]
    fn prop_sanitized_filename_never_starts_with_reserved_windows_name(raw in "\\PC*") {
        let clean = sanitize_filename(&raw);
        let uppercase = clean.to_ascii_uppercase();
        let reserved = ["CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "LPT1", "LPT2", "LPT3"];
        for r in reserved {
            prop_assert_ne!(&uppercase, r, "Cleaned filename must not match reserved name: {}", r);
            prop_assert!(!uppercase.starts_with(&format!("{r}.")), "Cleaned filename must not start with reserved prefix: {}", r);
        }
    }

    #[test]
    fn prop_safe_output_path_never_escapes_parent_dir(raw in "\\PC*") {
        let parent = Path::new("downloads");
        let path = safe_output_path(parent, &raw);
        prop_assert!(path.starts_with(parent), "Output path '{path:?}' must be confined to parent '{parent:?}'");
    }
}
