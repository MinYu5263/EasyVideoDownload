#[cfg(windows)]
pub(super) const OCCUPIED_FILE_OS_ERRORS: [u32; 4] = {
    use windows::Win32::Foundation::{
        ERROR_DELETE_PENDING, ERROR_LOCK_VIOLATION, ERROR_SHARING_VIOLATION, ERROR_USER_MAPPED_FILE,
    };
    [
        ERROR_SHARING_VIOLATION.0,
        ERROR_LOCK_VIOLATION.0,
        ERROR_DELETE_PENDING.0,
        ERROR_USER_MAPPED_FILE.0,
    ]
};

pub(crate) fn file_is_occupied(error: &std::io::Error) -> bool {
    #[cfg(windows)]
    {
        error
            .raw_os_error()
            .is_some_and(|code| OCCUPIED_FILE_OS_ERRORS.contains(&(code as u32)))
    }
    #[cfg(not(windows))]
    {
        let _ = error;
        false
    }
}

pub(super) fn failure_kind(code: &str, detail: &str, stage: &str) -> &'static str {
    match code {
        "toolMissing" | "toolSettingsFailed" | "ffmpegMissing" | "ffprobeMissing"
        | "denoMissing" => return "tools",
        "cookieReadFailed" | "cookieRequired" | "cookieSaveFailed" => return "cookie",
        "invalidDownloadDirectory"
        | "downloadDirectoryFailed"
        | "historyFileOccupied"
        | "historyFileDeleteFailed"
        | "historyFilePermissionDenied"
        | "historyFileReadOnly"
        | "historyFileChanged"
        | "historyFileUnsafe"
        | "historyFileInUse"
        | "historyFileDeletedSaveFailed"
        | "fileFailed" => return "filesystem",
        "invalidDownloadOptions" | "formatExpired" | "formatMismatch" => return "format",
        "verifyFailed" => return "processing",
        "downloadResultMissing" => return "output",
        "spawnFailed" | "readFailed" | "outputTooLarge" | "bridgeFailed" => return "execution",
        "downloadTimeout"
        | "proxySettingsFailed"
        | "networkFailed"
        | "httpFailed"
        | "signatureExpired"
        | "timeout"
        | "proxyFailed" => return "network",
        _ => {}
    }
    let text = detail.to_lowercase();
    if [
        "no space left",
        "disk full",
        "permission denied",
        "access is denied",
        "read-only file system",
    ]
        .iter()
        .any(|s| text.contains(s))
    {
        return "filesystem";
    }
    if [
        "requested format is not available",
        "requested format not available",
    ]
        .iter()
        .any(|s| text.contains(s))
    {
        return "format";
    }
    if [
        "video unavailable",
        "video has been removed",
        "video is private",
        "this video is private",
        "not available in your country",
    ]
        .iter()
        .any(|s| text.contains(s))
    {
        return "content";
    }
    if [
        "connection refused",
        "connection reset",
        "connection timed out",
        "name resolution",
        "network is unreachable",
        "unable to resolve host",
    ]
        .iter()
        .any(|s| text.contains(s))
    {
        return "network";
    }
    if [
        "cookies are no longer valid",
        "invalid cookies",
        "failed to load cookies",
        "could not read cookies",
    ]
        .iter()
        .any(|s| text.contains(s))
    {
        return "cookie";
    }
    if stage == "processing" {
        return "processing";
    }
    "unknown"
}

pub(crate) fn sanitize_diagnostic(detail: &str) -> String {
    let mut output = String::new();
    for line in detail.lines() {
        // Strip every URL (including signed media URLs and proxy credentials).
        let mut cleaned = String::new();
        let mut remainder = line;
        loop {
            let lower = remainder.to_ascii_lowercase();
            let start = ["https://", "http://", "socks5://", "socks5h://", "socks4://"]
                .iter()
                .filter_map(|prefix| lower.find(prefix))
                .min();
            let Some(start) = start else {
                cleaned.push_str(remainder);
                break;
            };
            cleaned.push_str(&remainder[..start]);
            cleaned.push_str("[URL]");
            let tail = &remainder[start..];
            let end = tail
                .find(|c: char| c.is_whitespace() || ['\'', '"', '<', '>'].contains(&c))
                .unwrap_or(tail.len());
            remainder = &tail[end..];
        }
        let lower = cleaned.to_lowercase();
        if [
            "cookie",
            "authorization",
            "authentication",
            "signature",
            "password",
            "passwd",
            "token",
            "secret",
            "api_key",
            "api-key",
            "apikey",
        ]
            .iter()
            .any(|key| lower.contains(key))
        {
            output.push_str("[redacted sensitive diagnostic]");
        } else {
            output.push_str(&cleaned);
        }
        output.push('\n');
    }
    output
        .chars()
        .take(4096)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classifications_require_specific_evidence() {
        for (code, kind) in [
            ("ffprobeMissing", "tools"),
            ("formatExpired", "format"),
            ("formatMismatch", "format"),
            ("fileFailed", "filesystem"),
            ("verifyFailed", "processing"),
            ("signatureExpired", "network"),
        ] {
            assert_eq!(failure_kind(code, "", "preparing"), kind);
        }
        assert_eq!(
            failure_kind("downloadFailed", "HTTP Error 403: Forbidden", "downloading"),
            "unknown"
        );
        assert_eq!(
            failure_kind(
                "downloadFailed",
                "Unable to download: Connection timed out",
                "downloading"
            ),
            "network"
        );
        assert_eq!(
            failure_kind(
                "downloadFailed",
                "Requested format is not available",
                "downloading"
            ),
            "format"
        );
        assert_eq!(
            failure_kind(
                "downloadFailed",
                "Postprocessing: ffmpeg exited",
                "processing"
            ),
            "processing"
        );
        assert_eq!(
            failure_kind("downloadFailed", "No space left on device", "processing"),
            "filesystem"
        );
        assert_eq!(failure_kind("cookieReadFailed", "", "preparing"), "cookie");
    }
    #[test]
    fn classification_survives_cookie_diagnostic_redaction() {
        let error =
            super::super::download_failure("ERROR: cookies are no longer valid: session=private");
        assert_eq!(error.failure_kind, Some("cookie"));
        assert!(!error.detail.contains("private"));
    }
    #[test]
    fn fresh_cookie_extractor_failure_reports_an_actionable_safe_error() {
        let error = super::super::download_failure(
            "ERROR: [Douyin] 123: Fresh cookies (not necessarily logged in) are needed; session=private",
        );
        assert_eq!(error.code, "cookieRequired");
        assert_eq!(error.failure_kind, Some("cookie"));
        assert_eq!(error.detail, "Platform authentication must be refreshed");
        assert!(!error.detail.contains("private"));
    }
    #[test]
    fn a_video_title_mentioning_fresh_cookies_is_not_an_authentication_error() {
        let error = super::super::download_failure(
            "[download] Destination: Fresh cookies recipe.mp4\nERROR: unable to download video data: HTTP Error 403",
        );
        assert_eq!(error.code, "downloadFailed");
        assert_eq!(error.failure_kind, None);
        let permission_error = super::super::download_failure(
            "ERROR: unable to open /Movies/Fresh Cookies.mp4.part: Permission denied",
        );
        assert_eq!(permission_error.code, "downloadFailed");
        assert_eq!(permission_error.failure_kind, Some("filesystem"));
    }
    #[test]
    fn diagnostic_redacts_credentials_cookie_values_and_media_urls() {
        let raw = "HTTP https://cdn.example/video?token=secret failed\nCookie: session=secret-cookie\nAuthorization: Bearer secret-auth\npassword=secret-password token=secret-token\ntrace connection refused";
        let safe = sanitize_diagnostic(raw);
        for secret in ["secret", "cdn.example", "session="] {
            assert!(!safe.contains(secret), "{safe}");
        }
        assert!(safe.contains("connection refused"));
    }
    #[test]
    fn diagnostic_redacts_signatures_and_authentication_headers_without_hiding_network_errors() {
        for text in ["signature=private", "X-Api-Key: private", "Authentication: private"] {
            assert!(!sanitize_diagnostic(text).contains("private"));
        }
        assert_eq!(sanitize_diagnostic("proxy connection refused"), "proxy connection refused");
    }
}

#[cfg(all(test, windows))]
mod occupied_file_tests {
    use super::*;
    #[test]
    fn only_explicit_windows_occupation_errors_are_classified_as_busy() {
        for code in [32, 33, 303, 1224] {
            assert!(file_is_occupied(&std::io::Error::from_raw_os_error(code)));
        }
        for code in [2, 3, 5, 112] {
            assert!(!file_is_occupied(&std::io::Error::from_raw_os_error(code)));
        }
        assert_eq!(
            failure_kind("historyFileOccupied", "", "finalizing"),
            "filesystem"
        );
    }
}
