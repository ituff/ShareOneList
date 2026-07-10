use url::Url;

/// Parses a SharePoint sharing URL and extracts a direct download link.
///
/// Supports URLs matching:
/// - `https://{tenant}.sharepoint.com/...` or `https://{tenant}.sharepoint.cn/...`
/// - Personal sharing patterns like `:w:/`, `:x:/`, `:p:/`, `:b:/`, `:f:/`
///   (Word, Excel, PowerPoint, PDF, Files respectively)
///
/// Returns the direct download URL in the format:
/// `https://{domain}/personal/{user}/_layouts/52/download.aspx?share={shareId}`
///
/// Returns None for non-SharePoint URLs, folder links (`:f:/`), or malformed URLs.
pub fn parse_sharepoint_url(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    let host = parsed.host_str()?;

    // Must be a SharePoint domain (.sharepoint.com or .sharepoint.cn)
    if !host.ends_with(".sharepoint.com") && !host.ends_with(".sharepoint.cn") {
        return None;
    }

    let path = parsed.path();

    // Try to match the short sharing URL pattern:
    // /:w:/p/{user}/{shareId} or /:x:/p/{user}/{shareId} etc.
    // The file type indicators are: :w: (Word), :x: (Excel), :p: (PowerPoint), :b: (PDF/binary), :f: (folder)
    if let Some(download_url) = parse_short_share_url(host, path) {
        return Some(download_url);
    }

    // Try to match the personal OneDrive layout URL with an id parameter:
    // /personal/{user}/_layouts/15/onedrive.aspx?...&id=...&...
    if let Some(download_url) = parse_personal_layout_url(host, path, &parsed) {
        return Some(download_url);
    }

    None
}

/// Parses short sharing URLs like:
/// `https://{tenant}-my.sharepoint.com/:w:/p/{user}/{shareId}`
/// `https://{tenant}-my.sharepoint.com/:x:/p/{user}/{shareId}`
///
/// The `:f:/` pattern indicates a folder link and returns None.
fn parse_short_share_url(host: &str, path: &str) -> Option<String> {
    // Match pattern: /:{type_char}:/p/{user}/{shareId}
    // or /:{type_char}:/g/{user}/{shareId} (guest sharing)
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    // Need at least: type_indicator, sharing_mode, user, shareId
    if segments.len() < 4 {
        return None;
    }

    let type_indicator = segments[0];

    // Must be a file type indicator pattern like :w:, :x:, :p:, :b:, :f:
    if !type_indicator.starts_with(':') || !type_indicator.ends_with(':') || type_indicator.len() != 3
    {
        return None;
    }

    let type_char = type_indicator.chars().nth(1)?;

    // :f: is a folder link — we don't support downloading folders
    if type_char == 'f' {
        return None;
    }

    // Valid file type indicators
    if !['w', 'x', 'p', 'b', 'u', 'o', 't', 'v'].contains(&type_char) {
        return None;
    }

    // segments[1] is the sharing mode: "p" (personal), "g" (guest), "r", "s", etc.
    let user = segments[2];
    let share_id = segments[3];

    // Build the download URL using the personal path format
    let domain = host;
    // Derive the base domain: if host is "{tenant}-my.sharepoint.com", keep it as is
    Some(format!(
        "https://{}/personal/{}/_layouts/52/download.aspx?share={}",
        domain, user, share_id
    ))
}

/// Parses personal OneDrive layout URLs like:
/// `https://{tenant}.sharepoint.com/personal/{user}/_layouts/15/onedrive.aspx?id=...`
///
/// Extracts the share parameter from query string if available.
fn parse_personal_layout_url(host: &str, path: &str, parsed: &Url) -> Option<String> {
    // Check if path contains /personal/{user}/
    let lower_path = path.to_lowercase();
    if !lower_path.contains("/personal/") {
        return None;
    }

    // Extract user from path: /personal/{user}/...
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let personal_idx = segments.iter().position(|s| s.eq_ignore_ascii_case("personal"))?;

    if personal_idx + 1 >= segments.len() {
        return None;
    }

    let user = segments[personal_idx + 1];

    // Look for share ID in query parameters (e.g., "id" or "share" parameter)
    // or "e" parameter which is the share token
    let share_id = parsed
        .query_pairs()
        .find(|(key, _)| key == "e" || key == "share")
        .map(|(_, value)| value.to_string());

    // If we have a share ID from query params, build the download URL
    if let Some(id) = share_id {
        return Some(format!(
            "https://{}/personal/{}/_layouts/52/download.aspx?share={}",
            host, user, id
        ));
    }

    // If there's an "id" parameter pointing to a file path, we can still use it
    // but we need a share token to build a proper download link
    let _file_id = parsed
        .query_pairs()
        .find(|(key, _)| key == "id")
        .map(|(_, value)| value.to_string());

    // Without a share token, we cannot construct a direct download link
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_share_url_word() {
        let url = "https://contoso-my.sharepoint.com/:w:/p/john_doe/EaBcDeFgHiJ";
        let result = parse_sharepoint_url(url);
        assert_eq!(
            result,
            Some("https://contoso-my.sharepoint.com/personal/john_doe/_layouts/52/download.aspx?share=EaBcDeFgHiJ".to_string())
        );
    }

    #[test]
    fn test_short_share_url_excel() {
        let url = "https://contoso-my.sharepoint.com/:x:/p/jane_smith/XyZaBcDeFgH";
        let result = parse_sharepoint_url(url);
        assert_eq!(
            result,
            Some("https://contoso-my.sharepoint.com/personal/jane_smith/_layouts/52/download.aspx?share=XyZaBcDeFgH".to_string())
        );
    }

    #[test]
    fn test_short_share_url_china() {
        let url = "https://contoso-my.sharepoint.cn/:b:/p/user1/AbCdEfGhIjK";
        let result = parse_sharepoint_url(url);
        assert_eq!(
            result,
            Some("https://contoso-my.sharepoint.cn/personal/user1/_layouts/52/download.aspx?share=AbCdEfGhIjK".to_string())
        );
    }

    #[test]
    fn test_folder_link_returns_none() {
        let url = "https://contoso-my.sharepoint.com/:f:/p/john_doe/EaBcDeFgHiJ";
        let result = parse_sharepoint_url(url);
        assert_eq!(result, None);
    }

    #[test]
    fn test_non_sharepoint_url_returns_none() {
        let url = "https://www.google.com/search?q=hello";
        let result = parse_sharepoint_url(url);
        assert_eq!(result, None);
    }

    #[test]
    fn test_personal_layout_url_with_share() {
        let url = "https://contoso-my.sharepoint.com/personal/john_doe/_layouts/15/onedrive.aspx?id=%2Fpersonal&e=AbCdEfGh";
        let result = parse_sharepoint_url(url);
        assert_eq!(
            result,
            Some("https://contoso-my.sharepoint.com/personal/john_doe/_layouts/52/download.aspx?share=AbCdEfGh".to_string())
        );
    }

    #[test]
    fn test_personal_layout_url_without_share_returns_none() {
        let url = "https://contoso-my.sharepoint.com/personal/john_doe/_layouts/15/onedrive.aspx?id=%2Fpersonal%2Fjohn_doe%2FDocuments%2Ffile.docx";
        let result = parse_sharepoint_url(url);
        assert_eq!(result, None);
    }

    #[test]
    fn test_malformed_url_returns_none() {
        let result = parse_sharepoint_url("not a url");
        assert_eq!(result, None);
    }
}
