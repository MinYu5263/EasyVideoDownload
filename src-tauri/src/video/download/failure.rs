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

pub(super) fn file_is_occupied(error: &std::io::Error) -> bool {
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
        "toolMissing" | "toolSettingsFailed" | "ffmpegMissing" | "denoMissing" => return "tools",
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
        | "historyFileDeletedSaveFailed" => return "filesystem",
        "invalidDownloadOptions" => return "format",
        "downloadResultMissing" => return "output",
        "spawnFailed" | "readFailed" | "outputTooLarge" | "bridgeFailed" => return "execution",
        "downloadTimeout" | "proxySettingsFailed" => return "network",
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

pub(super) fn sanitize_diagnostic(detail: &str) -> String {
    let mut output = String::new();
    for line in detail.lines() {
        // Strip every URL (including signed media URLs and proxy credentials).
        let mut cleaned = String::new();
        let mut remainder = line;
        loop {
            let lower = remainder.to_ascii_lowercase();
            let start = ["https://", "http://", "socks5://", "socks4://"]
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
            "password",
            "passwd",
            "token",
            "secret",
            "proxy",
            "api_key",
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
    fn diagnostic_redacts_credentials_cookie_values_and_media_urls() {
        let raw = "HTTP https://cdn.example/video?token=secret failed\nCookie: session=secret-cookie\nAuthorization: Bearer secret-auth\npassword=secret-password token=secret-token\ntrace connection refused";
        let safe = sanitize_diagnostic(raw);
        for secret in ["secret", "cdn.example", "session="] {
            assert!(!safe.contains(secret), "{safe}");
        }
        assert!(safe.contains("connection refused"));
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
