use crate::cookies::{CookiePlatform, CookieStore};
use crate::database::Storage;
use crate::proxy::{platform_proxy, ProxySettings};
#[cfg(test)]
use crate::required_tools::RequiredToolId;
use crate::required_tools::{process_tree, RequiredToolManager, RequiredToolSettings};
mod commands;
pub(crate) mod formats;
mod diagnostics;
pub mod download;
pub(crate) mod native_douyin;
use commands::parsing_command;
#[cfg(test)]
use commands::{
    command_preview, download_command, download_command_preview, render_command,
    DownloadCommandOptions,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(test)]
use std::path::Path;
use std::{io::Write, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoMetadata {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) thumbnail: Option<String>,
    pub(crate) duration: Option<f64>,
    pub(crate) extension: Option<String>,
    pub(crate) formats: Vec<VideoFormat>,
    #[serde(default)]
    pub(crate) default_format_id: Option<String>,
    pub(crate) cookie_fallback: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoFormat {
    pub(crate) format_id: String,
    pub(crate) height: Option<u32>,
    pub(crate) fps: Option<f64>,
    pub(crate) extension: Option<String>,
    pub(crate) size_bytes: Option<u64>,
    pub(crate) size_approximate: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) quality_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) quality_label_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) codec_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) video_codec: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) bitrate: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) watermarked: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) native_result_id: Option<String>,
    // Safe, bounded parser diagnostics live only until parse logging; never in IPC/SQLite.
    #[serde(skip)]
    pub(crate) raw_format_note: Option<String>,
    #[serde(skip)]
    pub(crate) raw_format_description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VideoError {
    code: String,
    detail: String,
    #[serde(rename = "storageError", skip_serializing_if = "Option::is_none")]
    storage_error: Option<crate::database::StorageError>,
    #[serde(skip)]
    failure_kind: Option<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedVideo {
    metadata: VideoMetadata,
    parsed_at: String,
    parser_fingerprint: String,
}
fn error(code: &str, detail: impl ToString) -> VideoError {
    VideoError {
        code: code.into(),
        detail: detail.to_string(),
        storage_error: None,
        failure_kind: None,
    }
}

pub(crate) fn normalize_link(input: &str, platform: CookiePlatform) -> Result<String, VideoError> {
    let text = if matches!(platform, CookiePlatform::Douyin) {
        let lower = input.to_ascii_lowercase();
        let start = [lower.find("https://"), lower.find("http://")]
            .into_iter()
            .flatten()
            .min()
            .ok_or_else(|| error("invalidLink", ""))?;
        input[start..]
            .split(|c: char| c.is_whitespace() || "<>\"'“”，。！？；、（）)".contains(c))
            .next()
            .unwrap_or("")
    } else {
        input.trim()
    };
    let url = url::Url::parse(text).map_err(|_| error("invalidLink", ""))?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(error("invalidLink", ""));
    }
    let domains: &[&str] = match platform {
        CookiePlatform::Douyin => &["douyin.com"],
        CookiePlatform::Bilibili => &["bilibili.com", "b23.tv"],
        CookiePlatform::Youtube => &["youtube.com", "youtu.be"],
    };
    let host = url.host_str().unwrap_or("");
    if !domains
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
    {
        return Err(error("platformMismatch", ""));
    }
    Ok(url.to_string())
}

fn nonempty(value: &Value, field: &str) -> Result<String, VideoError> {
    value[field]
        .as_str()
        .filter(|text| !text.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| error("invalidResult", format!("Missing {field}")))
}
fn positive(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .filter(|number| number.is_finite() && *number > 0.0)
}

fn byte_size(value: &Value) -> Option<u64> {
    // Keep serialized byte counts within JavaScript's exact integer range.
    positive(value)
        .filter(|bytes| *bytes >= 1.0 && *bytes <= 9_007_199_254_740_991.0)
        .map(|bytes| bytes.round() as u64)
}

fn parse_metadata(bytes: &[u8]) -> Result<VideoMetadata, VideoError> {
    let root: Value = serde_json::from_slice(bytes)
        .map_err(|_| error("invalidResult", "Invalid JSON metadata"))?;
    // --playlist-items 1 can leave only the first segment of a multi_video result.
    // Do not present that partial metadata as a complete video.
    if matches!(root["_type"].as_str(), Some("playlist" | "multi_video"))
        || root["entries"].is_array()
    {
        return Err(error(
            "unsupportedVideo",
            "Playlists and segmented videos are not supported yet",
        ));
    }
    let value = &root;
    let id = nonempty(value, "id")?;
    let title = nonempty(value, "title")?;
    let bilibili = value["extractor_key"]
        .as_str()
        .or_else(|| value["extractor"].as_str())
        .is_some_and(|name| name.to_ascii_lowercase().starts_with("bilibili"));
    let mut formats = value["formats"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|format| {
            // yt-dlp treats an unspecified codec as unknown, including Bilibili FLV.
            !matches!(format["vcodec"].as_str().unwrap_or("").trim().to_ascii_lowercase().as_str(), "none" | "images")
                && format["has_drm"].as_bool() != Some(true)
                && format["protocol"].as_str() != Some("mhtml")
        })
        .filter_map(|format| {
            let exact_size = byte_size(&format["filesize"]);
            let size_bytes = exact_size.or_else(|| byte_size(&format["filesize_approx"]));
            let note = format["format_note"].as_str().and_then(formats::meaningful);
            let platform_label = if bilibili {
                format["format"].as_str().and_then(formats::meaningful).filter(|label| {
                    !label.starts_with(&format!("{} - ", format["format_id"].as_str().unwrap_or_default()))
                })
            } else { None };
            let mut result = VideoFormat {
                format_id: format["format_id"]
                    .as_str()
                    .filter(|id| formats::valid_id(id))?
                    .into(),
                height: format["height"]
                    .as_u64()
                    .and_then(|height| u32::try_from(height).ok())
                    .filter(|height| *height > 0),
                fps: positive(&format["fps"]),
                extension: format["ext"]
                    .as_str()
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned),
                size_bytes,
                size_approximate: exact_size.is_none() && size_bytes.is_some(),
                width: format["width"]
                    .as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|n| *n > 0),
                quality_label: note.or(platform_label).map(str::to_owned),
                quality_label_source: if note.is_some() { Some("format_note") } else { platform_label.map(|_| "format") }.map(str::to_owned),
                video_codec: format["vcodec"]
                    .as_str()
                    .filter(|s| !s.is_empty() && *s != "none")
                    .map(str::to_owned),
                bitrate: positive(&format["vbr"])
                    .or_else(|| positive(&format["tbr"]))
                    .filter(|n| *n < 9_007_199_254_740.0)
                    .map(|n| (n * 1000.0).round() as u64),
                raw_format_note: format["format_note"].as_str().map(diagnostics::text),
                raw_format_description: format["format"].as_str().map(diagnostics::text),
                ..Default::default()
            };
            formats::normalize(&mut result);
            Some(result)
        })
        .collect::<Vec<_>>();
    if formats.is_empty() {
        return Err(error("noFormats", "No playable video formats returned"));
    }
    formats::normalize_all(&mut formats);
    let mut seen = std::collections::HashSet::new();
    formats.retain(|f| seen.insert(f.format_id.clone()));
    let thumbnail = value["thumbnail"]
        .as_str()
        .and_then(|text| url::Url::parse(text).ok())
        .filter(|url| {
            matches!(url.scheme(), "https" | "http")
                && url.username().is_empty()
                && url.password().is_none()
        })
        .map(|url| url.to_string());
    Ok(VideoMetadata {
        id,
        title,
        thumbnail,
        duration: value["duration"]
            .as_f64()
            .filter(|n| n.is_finite() && *n > 0.0),
        extension: value["ext"]
            .as_str()
            .filter(|text| !text.is_empty())
            .map(str::to_owned),
        default_format_id: formats::default_id(&formats),
        formats,
        cookie_fallback: false,
    })
}

fn cookie_snapshot(
    store: &CookieStore,
    platform: CookiePlatform,
) -> Result<Option<tempfile::NamedTempFile>, VideoError> {
    let contents = store
        .load(platform)
        .map_err(|e| error("cookieReadFailed", e.detail))?;
    cookie_copy(&contents)
}
fn cookie_copy(contents: &str) -> Result<Option<tempfile::NamedTempFile>, VideoError> {
    if contents.is_empty() {
        return Ok(None);
    }
    // yt-dlp writes its Cookie jar back on exit. Keep the user-maintained file untouched.
    let mut copy = tempfile::Builder::new()
        .prefix("evd-parse-cookie-")
        .tempfile()
        .map_err(|e| error("cookieReadFailed", e))?;
    copy.write_all(contents.as_bytes())
        .map_err(|e| error("cookieReadFailed", e))?;
    Ok(Some(copy))
}

async fn read_bounded(stream: impl AsyncRead + Unpin, maximum: u64) -> Result<Vec<u8>, VideoError> {
    let mut bytes = Vec::new();
    stream
        .take(maximum + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|e| error("readFailed", e))?;
    if bytes.len() as u64 > maximum {
        return Err(error("outputTooLarge", "yt-dlp output exceeded its limit"));
    }
    Ok(bytes)
}

async fn collect_metadata(mut command: Command, limit: Duration) -> Result<Vec<u8>, VideoError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let (mut child, tree) = process_tree::spawn(&mut command)
        .await
        .map_err(|e| error("spawnFailed", e))?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let result = tokio::time::timeout(limit, async {
        tokio::try_join!(
            async { child.wait().await.map_err(|e| error("readFailed", e)) },
            read_bounded(stdout, 8 * 1024 * 1024),
            read_bounded(stderr, 64 * 1024)
        )
    })
    .await;
    match result {
        Ok(Ok((status, stdout, stderr))) => {
            if !status.success() {
                let detail = String::from_utf8_lossy(&stderr);
                // Cookie format errors can quote credentials or whole lines. Do not expose them.
                let lower = detail.to_ascii_lowercase();
                if lower.contains("error: [youtube]")
                    && lower.contains("the page needs to be reloaded")
                {
                    return Err(error(
                        "youtubeReloadRequired",
                        "YouTube returned: The page needs to be reloaded.",
                    ));
                }
                if lower.contains("fresh cookies") {
                    return Err(error(
                        "cookieRequired",
                        "Fresh platform cookies are required",
                    ));
                }
                let detail = if lower.contains("cookie") || detail.contains("Traceback") {
                    "yt-dlp failed; check the platform Cookie file and tool version".into()
                } else {
                    crate::app_logs::safe_text(&detail)
                };
                return Err(error(
                    "parseFailed",
                    if detail.trim().is_empty() {
                        status.to_string()
                    } else {
                        detail
                    },
                ));
            }
            Ok(stdout)
        }
        failure => {
            drop(tree);
            let _ = child.start_kill();
            let _ = child.wait().await;
            Err(match failure {
                Ok(Err(e)) => e,
                Err(_) => error("timeout", "90 seconds"),
                _ => unreachable!(),
            })
        }
    }
}

async fn parse_with_cookie_retry<F, Fut>(
    platform: CookiePlatform,
    has_cookie: bool,
    mut attempt: F,
) -> Result<VideoMetadata, VideoError>
where
    F: FnMut(bool) -> Fut,
    Fut: std::future::Future<Output = Result<VideoMetadata, VideoError>>,
{
    match attempt(has_cookie).await {
        Err(failure)
            if matches!(platform, CookiePlatform::Youtube)
                && has_cookie
                && failure.code == "youtubeReloadRequired" =>
        {
            // This signed-in player failure also affects public videos. Retry natively,
            // without the Cookie snapshot; never change the managed Cookie configuration.
            match attempt(false).await {
                Ok(mut video) => {
                    video.cookie_fallback = true;
                    Ok(video)
                }
                Err(mut failure) => {
                    failure.detail = format!(
                        "Signed-in YouTube parsing was rejected. Public parsing also failed: {}",
                        failure.detail
                    );
                    Err(failure)
                }
            }
        }
        result => result,
    }
}

async fn parse_with_tools(
    settings: RequiredToolSettings,
    store: CookieStore,
    platform: CookiePlatform,
    input: &str,
    proxy: Option<ProxySettings>,
) -> Result<VideoMetadata, VideoError> {
    let url = normalize_link(input, platform)?;
    let copy = tauri::async_runtime::spawn_blocking(move || cookie_snapshot(&store, platform))
        .await
        .map_err(|e| error("cookieReadFailed", e))??;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
    // Both attempts share one timeout budget. Keep the snapshot alive through both.
    parse_with_cookie_retry(platform, copy.is_some(), |use_cookie| {
        let cookie = if use_cookie {
            copy.as_ref().map(|file| file.path())
        } else {
            None
        };
        let command = parsing_command(&settings, &url, cookie, proxy.as_ref());
        async move {
            let bytes = collect_metadata(
                command?,
                deadline.saturating_duration_since(tokio::time::Instant::now()),
            )
            .await?;
            parse_metadata(&bytes)
        }
    })
    .await
}

#[tauri::command]
pub async fn parse_video(
    app: tauri::AppHandle,
    platform: CookiePlatform,
    input: String,
    tools: tauri::State<'_, RequiredToolManager>,
    cookies: tauri::State<'_, CookieStore>,
    storage: tauri::State<'_, Storage>,
) -> Result<ParsedVideo, VideoError> {
    let started = std::time::Instant::now();
    let request_id = uuid::Uuid::new_v4().to_string();
    let platform_name = match platform {
        CookiePlatform::Douyin => "douyin",
        CookiePlatform::Bilibili => "bilibili",
        CookiePlatform::Youtube => "youtube",
    };
    crate::app_logs::record(
        &app,
        crate::app_logs::entry(
            "info",
            platform_name,
            None,
            Some(&request_id),
            "parseStarted",
            None,
            String::new(),
        ),
    );
    let result: Result<ParsedVideo, VideoError> = async {
        let (settings, _tool_usage) = tools
            .settings_and_usage()
            .map_err(|e| error("toolSettingsFailed", e.detail))?;
        let proxy = platform_proxy(storage.inner(), platform)
            .await
            .map_err(|e| error("proxySettingsFailed", e.detail))?;
        crate::app_logs::record(
            &app,
            crate::app_logs::entry(
                "info",
                platform_name,
                None,
                Some(&request_id),
                "parseNetworkConfigured",
                None,
                diagnostics::network_summary(platform_name, proxy.as_ref()),
            ),
        );
        let parser_fingerprint =
            crate::database::page_states::parser_fingerprint_for_platform(&settings, platform_name)
                .map_err(|e| error("toolSettingsFailed", e.detail))?;
        let metadata = if matches!(platform, CookiePlatform::Douyin) {
            let contents = cookies
                .load(platform)
                .map_err(|_| error("cookieReadFailed", "Unable to read Douyin Cookie"))?;
            native_douyin::parse(&contents, proxy.as_ref(), &input).await?
        } else {
            parse_with_tools(settings, cookies.inner().clone(), platform, &input, proxy).await?
        };
        Ok(ParsedVideo {
            metadata,
            parsed_at: crate::datetime::now(),
            parser_fingerprint,
        })
    }
        .await;
    match &result {
        Ok(parsed) => {
            for entry in diagnostics::completed(
                platform_name,
                &request_id,
                &parsed.metadata,
                started.elapsed(),
            ) {
                crate::app_logs::record(&app, entry);
            }
        }
        Err(error) => crate::app_logs::record(
            &app,
            crate::app_logs::entry(
                "error",
                platform_name,
                None,
                Some(&request_id),
                "parseFailed",
                Some(&error.code),
                error.detail.clone(),
            ),
        ),
    }
    result
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod proxy_live_tests;
