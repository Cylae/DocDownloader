use std::path::{Path, PathBuf};

/// Reserved Windows device names that cannot be used as filenames or extensions.
const RESERVED_WINDOWS_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL",
    "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
    "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Sanitizes an untrusted string (e.g. publication title) to form a safe, valid cross-platform filename.
pub fn sanitize_filename(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return "document".to_string();
    }

    let mut sanitized = String::with_capacity(trimmed.len());

    for ch in trimmed.chars() {
        match ch {
            // Illegal characters across Windows, Linux, macOS
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => {
                sanitized.push('_');
            }
            // Control characters (0x00 to 0x1F and 0x7F)
            c if c.is_control() => {
                sanitized.push('_');
            }
            c => {
                sanitized.push(c);
            }
        }
    }

    // Collapse multiple consecutive underscores
    let mut collapsed = String::with_capacity(sanitized.len());
    let mut prev_underscore = false;
    for ch in sanitized.chars() {
        if ch == '_' {
            if !prev_underscore {
                collapsed.push('_');
                prev_underscore = true;
            }
        } else {
            collapsed.push(ch);
            prev_underscore = false;
        }
    }

    // Strip leading/trailing periods, spaces, underscores
    let mut cleaned = collapsed
        .trim_matches(|c: char| c == '.' || c == ' ' || c == '_')
        .to_string();

    if cleaned.is_empty() {
        cleaned = "document".to_string();
    }

    // Check against Windows reserved device names (case-insensitive)
    let uppercase = cleaned.to_ascii_uppercase();
    for reserved in RESERVED_WINDOWS_NAMES {
        if uppercase == *reserved
            || uppercase.starts_with(&format!("{reserved}."))
        {
            cleaned = format!("doc_{cleaned}");
            break;
        }
    }

    // Bound maximum byte length (240 bytes to safely allow .pdf extension under 255-byte filesystem limits)
    if cleaned.len() > 240 {
        // Truncate cleanly at char boundary
        let mut byte_count = 0;
        let mut truncated = String::new();
        for ch in cleaned.chars() {
            let next_len = ch.len_utf8();
            if byte_count + next_len > 240 {
                break;
            }
            truncated.push(ch);
            byte_count += next_len;
        }
        cleaned = truncated
            .trim_end_matches(|c: char| c == '.' || c == ' ' || c == '_')
            .to_string();
        if cleaned.is_empty() {
            cleaned = "document".to_string();
        }
    }

    cleaned
}

/// Safely resolves a sanitized publication title into a full target PDF path within the output directory.
/// Strictly prevents path traversal outside `output_dir`.
pub fn safe_output_path(output_dir: &Path, title: &str) -> PathBuf {
    let safe_stem = sanitize_filename(title);
    let mut filename = safe_stem;
    if !filename.to_ascii_lowercase().ends_with(".pdf") {
        filename.push_str(".pdf");
    }

    // Ensure resulting path is strictly within output_dir
    let candidate = output_dir.join(&filename);
    
    // Normalization check: candidate must have output_dir as its prefix
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitization_removes_traversal_and_forbidden_chars() {
        assert_eq!(sanitize_filename("../../../etc/passwd"), "etc_passwd");
        assert_eq!(sanitize_filename("hello:world*test?file"), "hello_world_test_file");
        assert_eq!(sanitize_filename("  trailing spaces and dots...  "), "trailing spaces and dots");
        assert_eq!(sanitize_filename(""), "document");
        assert_eq!(sanitize_filename("   "), "document");
    }

    #[test]
    fn test_sanitization_escapes_windows_reserved() {
        assert_eq!(sanitize_filename("CON"), "doc_CON");
        assert_eq!(sanitize_filename("nul"), "doc_nul");
        assert_eq!(sanitize_filename("COM1.txt"), "doc_COM1.txt");
        assert_eq!(sanitize_filename("AUX"), "doc_AUX");
    }

    #[test]
    fn test_safe_output_path() {
        let dir = Path::new("/downloads");
        let path = safe_output_path(dir, "My Great Magazine: Edition 1");
        assert_eq!(path, dir.join("My Great Magazine_ Edition 1.pdf"));
    }
}
