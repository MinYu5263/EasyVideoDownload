use super::download::{history::DownloadRecordSession, DownloadProgress, DownloadResult};
use super::{error, VideoError, VideoFormat, VideoMetadata};
use crate::{
    database::page_states::DownloadPageState,
    douyin::{Candidate, ParsedResult},
};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::Duration,
};

static CACHE: OnceLock<Mutex<VecDeque<ParsedResult>>> = OnceLock::new();
fn cache() -> &'static Mutex<VecDeque<ParsedResult>> {
    CACHE.get_or_init(Default::default)
}
fn safe_error(e: crate::douyin::LabError) -> VideoError {
    let mut failure = error(
        match e.code {
            "cancelled" => "downloadCancelled",
            "cookieMissing" => "cookieRequired",
            "unsupported" => "unsupportedVideo",
            _ => e.code,
        },
        e.detail,
    );
    failure.failure_kind = Some(match e.code {
        "ffprobeMissing" => "tools",
        "cookieMissing" => "cookie",
        "formatExpired" | "formatMismatch" => "format",
        "fileFailed" => "filesystem",
        "verifyFailed" => "processing",
        "networkFailed" | "httpFailed" | "timeout" | "signatureExpired" | "downloadFailed"
        | "proxyFailed" => "network",
        "unsupported" | "emptyResponse" => "content",
        _ => "unknown",
    });
    failure
}
async fn await_transfer<T>(
    future: impl std::future::Future<Output=Result<T, crate::douyin::LabError>>,
    mut cancel: tokio::sync::watch::Receiver<bool>,
    token: &crate::douyin::cancel::Cancellation,
    limit: Duration,
) -> Result<T, VideoError> {
    tokio::pin!(future);
    let result = tokio::select! {
        result = &mut future => result,
        _ = async {
            loop {
                if *cancel.borrow_and_update() { break; }
                if cancel.changed().await.is_err() { std::future::pending::<()>().await; }
            }
        } => {
            token.cancel();
            future.await
        },
        _ = token.active_timeout(limit) => {
            token.cancel();
            // Publication may have won the race; retain its successful result.
            match future.await {
                Ok(output) => Ok(output),
                Err(_) => return Err(error("downloadTimeout", "Download exceeded six hours")),
            }
        },
    };
    result.map_err(safe_error)
}
fn format(parsed: &ParsedResult, candidate: &Candidate) -> VideoFormat {
    let f = &candidate.format;
    let mut result = VideoFormat {
        format_id: f.id.clone(),
        height: f.height,
        fps: f.fps,
        extension: Some("mp4".into()),
        size_bytes: f.file_size,
        size_approximate: false,
        width: f.width,
        video_codec: Some(f.codec.clone()),
        bitrate: f.bitrate,
        watermarked: f.watermarked,
        native_result_id: Some(parsed.video.result_id.clone()),
        ..Default::default()
    };
    super::formats::normalize(&mut result);
    result
}
fn metadata(parsed: &ParsedResult) -> VideoMetadata {
    let mut formats: Vec<_> = parsed.candidates.iter().map(|c| format(parsed, c)).collect();
    super::formats::normalize_all(&mut formats);
    VideoMetadata {
        id: parsed.video.video_id.clone(),
        title: parsed.video.title.clone(),
        thumbnail: parsed.video.cover.clone(),
        duration: parsed.video.duration,
        extension: Some("mp4".into()),
        default_format_id: super::formats::default_id(&formats),
        formats,
        cookie_fallback: false,
    }
}
pub(crate) async fn parse(
    contents: &str,
    proxy: Option<&crate::proxy::ProxySettings>,
    input: &str,
) -> Result<VideoMetadata, VideoError> {
    let client = crate::douyin::http::client(contents, proxy).map_err(safe_error)?;
    let parsed = crate::douyin::http::parse_video(&client, input)
        .await
        .map_err(safe_error)?;
    remember(parsed)
}
pub(crate) fn remember(parsed: ParsedResult) -> Result<VideoMetadata, VideoError> {
    let result = metadata(&parsed);
    let mut entries = cache()
        .lock()
        .map_err(|_| error("parseFailed", "Result cache unavailable"))?;
    entries.push_back(parsed);
    while entries.len() > 32 {
        entries.pop_front();
    }
    Ok(result)
}
fn codec(value: &str) -> &str {
    if value.starts_with("bytevc1")
        || value.starts_with("hev")
        || value.starts_with("hvc")
        || value == "h265"
    {
        "hevc"
    } else if value.starts_with("avc") || value == "h264" {
        "h264"
    } else {
        value
    }
}
fn match_candidate(
    parsed: &ParsedResult,
    selected: &VideoFormat,
    recorded: bool,
) -> Result<Candidate, VideoError> {
    let expired = || {
        error(
            "formatExpired",
            "Selected format changed or is ambiguous; parse and select again",
        )
    };
    let matches_fields = |c: &Candidate| {
        let f = &c.format;
        selected.width.is_none_or(|v| f.width == Some(v))
            && selected.height.is_none_or(|v| f.height == Some(v))
            && selected.fps.is_none_or(|v| f.fps == Some(v))
            && selected
            .video_codec
            .as_deref()
            .is_none_or(|v| codec(v) == f.codec)
            && selected.bitrate.is_none_or(|v| f.bitrate == Some(v))
            && selected
            .watermarked
            .is_none_or(|v| f.watermarked == Some(v))
            && selected.extension.as_deref().is_none_or(|v| v == "mp4")
    };
    if let Some(c) = parsed
        .candidates
        .iter()
        .find(|c| c.format.id == selected.format_id)
    {
        return matches_fields(c).then(|| c.clone()).ok_or_else(expired);
    }
    if !recorded {
        return Err(expired());
    }
    // Old yt-dlp IDs encode codec, resolution and bit rate; ignore only the mirror suffix.
    let legacy = selected
        .format_id
        .rsplit_once('-')
        .map(|(base, _)| base)
        .unwrap_or(&selected.format_id);
    let parts = legacy.split('_').collect::<Vec<_>>();
    let legacy_spec = if parts.len() == 3 && matches!(parts[0], "bytevc1" | "h264" | "avc") {
        Some((
            codec(parts[0]),
            parts[1]
                .strip_suffix('p')
                .and_then(|v| v.parse::<u32>().ok()),
            parts[2].parse::<u64>().ok(),
        ))
    } else {
        None
    };
    let mut matches = parsed.candidates.iter().filter(|c| {
        matches_fields(c)
            && match legacy_spec {
            Some((codec, Some(height), Some(rate))) => {
                selected.height == Some(height)
                    && selected.fps.is_some()
                    && c.format.codec == codec
                    && c.format.bitrate == Some(rate)
                    && c.format.height == Some(height)
            }
            Some(_) => false,
            None => {
                selected.native_result_id.is_some()
                    && selected.width.is_some()
                    && selected.height.is_some()
                    && selected.fps.is_some()
                    && selected.bitrate.is_some()
                    && selected
                    .video_codec
                    .as_deref()
                    .is_some_and(|v| v != "unknown")
            }
        }
    });
    let candidate = matches.next().cloned().ok_or_else(expired)?;
    if matches.next().is_some() {
        return Err(expired());
    }
    Ok(candidate)
}
pub(crate) struct NativeExecution {
    client: reqwest::Client,
    parsed: ParsedResult,
    candidate: Candidate,
    ffprobe: PathBuf,
}
impl NativeExecution {
    pub(crate) fn check_tools(&self) -> Result<(), VideoError> {
        if self.ffprobe.is_file() {
            Ok(())
        } else {
            Err(error("ffprobeMissing", "Configure ffprobe in Settings"))
        }
    }
    pub(crate) async fn run(
        self,
        cancel: tokio::sync::watch::Receiver<bool>,
        directory: &Path,
        temporary_directory: &Path,
        notify: impl Fn(DownloadProgress) -> Result<(), VideoError> + Sync,
        history: &DownloadRecordSession,
        token: crate::douyin::cancel::Cancellation,
    ) -> Result<DownloadResult, VideoError> {
        if *cancel.borrow() {
            return Err(error("downloadCancelled", ""));
        }
        let notification_error = Mutex::new(None);
        let emit = |p| {
            if let Err(e) = notify(p) {
                *notification_error.lock().unwrap() = Some(e);
                token.cancel();
            }
        };
        history.native_stage("downloading");
        let started = token.active_elapsed();
        let transfer = crate::douyin::download::download(
            &self.client,
            &self.parsed,
            &self.candidate,
            directory,
            temporary_directory,
            &self.ffprobe,
            &token,
            |bytes, total| {
                let speed = bytes as f64
                    / token
                    .active_elapsed()
                    .saturating_sub(started)
                    .as_secs_f64()
                    .max(0.001);
                emit(DownloadProgress {
                    phase: "downloading",
                    percent: total
                        .filter(|n| *n > 0)
                        .map(|n| (bytes as f64 / n as f64 * 100.0).min(100.0)),
                    speed: (bytes > 0).then_some(speed),
                    eta: total
                        .filter(|_| speed > 0.0)
                        .map(|n| n.saturating_sub(bytes) as f64 / speed),
                });
            },
            || {
                history.native_stage("processing");
                emit(DownloadProgress {
                    phase: "processing",
                    percent: None,
                    speed: None,
                    eta: None,
                });
            },
        );
        let result =
            await_transfer(transfer, cancel, &token, Duration::from_secs(6 * 60 * 60)).await;
        if let Some(e) = notification_error.lock().unwrap().take() {
            return Err(e);
        }
        let (path, _) = result?;
        history.native_stage("finalizing");
        Ok(DownloadResult {
            path,
            already_downloaded: false,
            storage_error: None,
        })
    }
}
pub(crate) async fn capture(
    page: &mut DownloadPageState,
    contents: &str,
    proxy: Option<&crate::proxy::ProxySettings>,
    recorded: bool,
    ffprobe: PathBuf,
) -> Result<NativeExecution, VideoError> {
    let selected = page
        .formats
        .iter()
        .find(|f| Some(&f.format_id) == page.selected_format_id.as_ref())
        .cloned()
        .ok_or_else(|| error("invalidDownloadOptions", "Missing selected format"))?;
    let client = crate::douyin::http::client(contents, proxy).map_err(safe_error)?;
    let cached = if recorded {
        // A history retry must validate current platform formats before old-file deletion.
        None
    } else {
        let entries = cache()
            .lock()
            .map_err(|_| error("parseFailed", "Result cache unavailable"))?;
        entries
            .iter()
            .find(|p| Some(&p.video.result_id) == selected.native_result_id.as_ref())
            .cloned()
    };
    let parsed = if let Some(p) = cached {
        p
    } else {
        crate::douyin::http::parse_video(&client, &page.input_link)
            .await
            .map_err(safe_error)?
    };
    if page.video_id.as_deref() != Some(&parsed.video.video_id) {
        return Err(error(
            "formatExpired",
            "Video identity changed; parse again",
        ));
    }
    let candidate = match_candidate(&parsed, &selected, recorded)?;
    // Pin the parsed URLs in this task, independently of the bounded page cache.
    if recorded {
        let f = format(&parsed, &candidate);
        page.selected_format_id = Some(f.format_id.clone());
        page.selected_height = f.height;
        page.selected_fps = f.fps;
        page.formats = vec![f];
    }
    let execution = NativeExecution {
        client,
        parsed,
        candidate,
        ffprobe,
    };
    execution.check_tools()?;
    Ok(execution)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_failures_use_formal_codes_and_history_categories() {
        let unsupported = safe_error(crate::douyin::LabError::new(
            "unsupported",
            "Images are not supported",
        ));
        assert_eq!(unsupported.code, "unsupportedVideo");
        assert_eq!(unsupported.failure_kind, Some("content"));
        let cookie = safe_error(crate::douyin::LabError::new(
            "cookieMissing",
            "Import cookies",
        ));
        assert_eq!(cookie.code, "cookieRequired");
        assert_eq!(cookie.failure_kind, Some("cookie"));
        let format = safe_error(crate::douyin::LabError::new("formatExpired", "Parse again"));
        assert_eq!(format.failure_kind, Some("format"));
    }
    fn parsed() -> ParsedResult {
        crate::douyin::parse_detail(&serde_json::json!({"aweme_detail":{"aweme_id":"123","desc":"test","video":{"bit_rate":[
            {"is_bytevc1":true,"bit_rate":800,"FPS":30,"play_addr":{"width":1920,"height":1080,"data_size":1000,"url_list":["https://private.test/a"]}},
            {"is_bytevc1":false,"bit_rate":1200,"FPS":30,"play_addr":{"width":1920,"height":1080,"data_size":2000,"url_list":["https://private.test/b"]}}
        ]}}})).unwrap()
    }
    #[tokio::test]
    async fn cancellation_after_native_publication_keeps_successful_output() {
        let root = tempfile::tempdir().unwrap();
        eprintln!(
            "owned native publication race directory: {}",
            root.path().display()
        );
        let output = root.path().join("completed.mp4");
        let token = crate::douyin::cancel::Cancellation::default();
        let (signal, cancel) = tokio::sync::watch::channel(false);
        let future = async {
            token.commit(|| {
                std::fs::write(&output, b"published output")
                    .map_err(|_| crate::douyin::LabError::new("fileFailed", "Write failed"))
            })?;
            signal.send_replace(true);
            tokio::task::yield_now().await;
            Ok(output.clone())
        };
        assert_eq!(
            await_transfer(future, cancel, &token, Duration::from_secs(5))
                .await
                .unwrap(),
            output
        );
        assert_eq!(std::fs::read(&output).unwrap(), b"published output");
    }
    #[test]
    fn legacy_formats_match_codec_resolution_rate_and_reject_ambiguous_variants() {
        let mut p = parsed();
        let f: VideoFormat = serde_json::from_value(serde_json::json!({"formatId":"bytevc1_1080p_800-3","height":1080,"fps":30,"extension":"mp4","sizeBytes":1000,"sizeApproximate":false})).unwrap();
        assert_eq!(match_candidate(&p, &f, true).unwrap().format.codec, "hevc");
        let mut other = p
            .candidates
            .iter()
            .find(|c| c.format.codec == "hevc")
            .unwrap()
            .clone();
        other.format.id = "other".into();
        p.candidates.push(other);
        assert_eq!(
            match_candidate(&p, &f, true).err().unwrap().code,
            "formatExpired"
        );
        assert!(match_candidate(&p, &f, false).is_err());
    }
}
