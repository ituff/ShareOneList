//! File name validation for pre-flight checks before Graph API calls.
//!
//! Mirrors the frontend validation in `src/lib/validators.ts` to catch
//! invalid names early on the Rust side before making network requests.

/// Characters not allowed in OneDrive/SharePoint file or folder names.
const INVALID_CHARS: &[char] = &['\\', '/', ':', '*', '?', '"', '<', '>', '|'];

/// Maximum allowed file name length.
const MAX_NAME_LENGTH: usize = 400;

/// Validate a file or folder name against OneDrive/SharePoint naming rules.
///
/// # Rules
/// - Must be between 1 and 400 characters
/// - Must not contain: `\ / : * ? " < > |`
///
/// # Returns
/// `Ok(())` if valid, `Err(String)` with a descriptive error message if invalid.
///
/// # Examples
/// ```
/// use share_one_list_lib::graph::validators::validate_file_name;
///
/// assert!(validate_file_name("valid-file.txt").is_ok());
/// assert!(validate_file_name("").is_err());
/// assert!(validate_file_name("file:name").is_err());
/// ```
pub fn validate_file_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("File name cannot be empty".to_string());
    }

    if name.len() > MAX_NAME_LENGTH {
        return Err(format!(
            "File name must be {} characters or fewer",
            MAX_NAME_LENGTH
        ));
    }

    if let Some(ch) = name.chars().find(|c| INVALID_CHARS.contains(c)) {
        return Err(format!("File name contains invalid character: {}", ch));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_names() {
        assert!(validate_file_name("document.txt").is_ok());
        assert!(validate_file_name("my file (1).docx").is_ok());
        assert!(validate_file_name("a").is_ok());
        assert!(validate_file_name("日本語ファイル.pdf").is_ok());
        assert!(validate_file_name("file-name_v2.0.tar.gz").is_ok());
    }

    #[test]
    fn test_empty_name() {
        let result = validate_file_name("");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "File name cannot be empty");
    }

    #[test]
    fn test_max_length_boundary() {
        // Exactly 400 chars should be valid
        let name_400 = "a".repeat(400);
        assert!(validate_file_name(&name_400).is_ok());

        // 401 chars should be invalid
        let name_401 = "a".repeat(401);
        let result = validate_file_name(&name_401);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("400 characters or fewer"));
    }

    #[test]
    fn test_invalid_characters() {
        let invalid_names = vec![
            ("file\\name", '\\'),
            ("file/name", '/'),
            ("file:name", ':'),
            ("file*name", '*'),
            ("file?name", '?'),
            ("file\"name", '"'),
            ("file<name", '<'),
            ("file>name", '>'),
            ("file|name", '|'),
        ];

        for (name, expected_char) in invalid_names {
            let result = validate_file_name(name);
            assert!(result.is_err(), "Expected '{}' to be invalid", name);
            let err = result.unwrap_err();
            assert!(
                err.contains(&expected_char.to_string()),
                "Error for '{}' should mention '{}', got: {}",
                name,
                expected_char,
                err
            );
        }
    }

    #[test]
    fn test_multiple_invalid_chars_reports_first() {
        // Should report the first invalid character encountered
        let result = validate_file_name("a:b*c");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains(':'));
    }

    #[test]
    fn test_whitespace_only_name_is_valid() {
        // Whitespace-only names pass character validation (server may reject them)
        assert!(validate_file_name("   ").is_ok());
    }
}
