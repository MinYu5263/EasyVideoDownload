#[cfg(test)]
use super::normalize_link;
use super::{error, VideoError};
#[cfg(test)]
use crate::cookies::{CookiePlatform, CookieStore};
use crate::proxy::ProxySettings;
use crate::required_tools::{RequiredToolId, RequiredToolSettings};
use serde::Deserialize;
#[cfg(test)]
use serde::Serialize;
use std::path::Path;
use tokio::process::Command;

fn configured_command(
    settings: &RequiredToolSettings,
    cookie: Option<&Path>,
    purpose: &[&str],
    proxy: Option<&ProxySettings>,
) -> Result<Command, VideoError> {
    let ytdlp = settings
        .tools
        .get(&RequiredToolId::Ytdlp)
        .and_then(|tool| tool.programs.first())
        .ok_or_else(|| error("toolMissing", "yt-dlp"))?;
    let mut command = Command::new(&ytdlp.path);
    // Keep command options independent of user configs and plugins.
    command.args([
        "--ignore-config",
        "--no-plugin-dirs",
        "--no-playlist",
        "--playlist-items",
        "1",
        "--no-warnings",
        "--no-cache-dir",
        "--encoding",
        "utf-8",
        "--socket-timeout",
        "20",
        "--retries",
        "1",
        "--extractor-retries",
        "1",
    ]);
    if let Some(proxy) = proxy {
        // Only an explicit app proxy overrides the original network environment.
        // With the switch off, yt-dlp keeps its normal system/VPN proxy discovery.
        for name in [
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "FTP_PROXY",
            "NO_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
            "ftp_proxy",
            "no_proxy",
        ] {
            command.env_remove(name);
        }
        let url = proxy.url();
        command.arg("--proxy").arg(&url);
        // A nonempty environment proxy map avoids Windows registry bypass rules.
        for name in [
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
        ] {
            command.env(name, &url);
        }
    }
    command.args(purpose);
    if let Some(deno) = settings
        .tools
        .get(&RequiredToolId::Deno)
        .and_then(|tool| tool.programs.first())
    {
        let mut runtime = std::ffi::OsString::from("deno:");
        runtime.push(&deno.path);
        command
            .args(["--no-js-runtimes", "--js-runtimes"])
            .arg(runtime);
    }
    if let Some(path) = cookie {
        command.arg("--cookies").arg(path);
    }
    Ok(command)
}

pub(super) fn parsing_command(
    settings: &RequiredToolSettings,
    url: &str,
    cookie: Option<&Path>,
    proxy: Option<&ProxySettings>,
) -> Result<Command, VideoError> {
    let mut command = configured_command(
        settings,
        cookie,
        &["--dump-single-json", "--simulate", "--no-progress"],
        proxy,
    )?;
    command.arg("--").arg(url);
    Ok(command)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadCommandOptions {
    pub(super) directory: String,
    pub(super) format_id: String,
    pub(super) container: Option<String>,
    #[serde(default)]
    pub(super) cookie_fallback: bool,
    // Only an immutable native parse/record snapshot supplies format constraints.
    #[serde(skip)]
    pub(super) selected_format: Option<super::VideoFormat>,
}

pub(super) fn download_command(
    settings: &RequiredToolSettings,
    url: &str,
    cookie: Option<&Path>,
    options: &DownloadCommandOptions,
    proxy: Option<&ProxySettings>,
) -> Result<Command, VideoError> {
    download_command_with_temporary_directory(settings, url, cookie, options, proxy, None)
}

pub(super) fn download_command_with_temporary_directory(
    settings: &RequiredToolSettings,
    url: &str,
    cookie: Option<&Path>,
    options: &DownloadCommandOptions,
    proxy: Option<&ProxySettings>,
    temporary_directory: Option<&Path>,
) -> Result<Command, VideoError> {
    if !Path::new(&options.directory).is_absolute() {
        return Err(error(
            "invalidDownloadDirectory",
            "Choose an absolute output directory",
        ));
    }
    if !super::formats::valid_id(&options.format_id) {
        return Err(error("invalidDownloadOptions", "Invalid native format ID"));
    }
    if options
        .container
        .as_deref()
        .is_some_and(|container| container != "mp4")
    {
        return Err(error(
            "invalidDownloadOptions",
            "Unsupported output container",
        ));
    }
    let mut command = configured_command(
        settings,
        if options.cookie_fallback {
            None
        } else {
            cookie
        },
        &[
            "--no-simulate",
            "--verbose",
            "--color",
            "never",
            "--newline",
            "--no-overwrites",
            "--progress",
            "--progress-delta",
            "0.3",
            "--progress-template",
            "download:__EVD_PROGRESS__{\"formatId\":%(info.format_id)j,\"progress\":%(progress.{status,downloaded_bytes,total_bytes,total_bytes_estimate,speed,eta})j}",
            "--progress-template",
            "postprocess:__EVD_PROCESSING__%(progress.{status,postprocessor})j",
            "--print",
            "before_dl:__EVD_PLAN__{\"filepath\":%(_filename)j,\"formats\":%(requested_formats.:.{format_id,filesize,filesize_approx}|[])j,\"formatId\":%(format_id)j,\"size\":%(filesize,filesize_approx|0)j}",
            "--print",
            "after_move:__EVD_FILE__%(.{filepath,__real_download})j",
        ],
        proxy,
    )?;
    let ffmpeg = settings
        .tools
        .get(&RequiredToolId::Ffmpeg)
        .and_then(|tool| {
            tool.programs
                .iter()
                .find(|program| program.name == "ffmpeg")
        })
        .ok_or_else(|| error("ffmpegMissing", "FFmpeg"))?;
    // Match the exact selected stream. Quoting keeps numeric IDs as strings.
    // Video-only streams use separate audio when present. The second branch also
    // handles combined streams and genuinely silent videos without forcing audio.
    let format = download_format_selector(options, url);
    if let Some(container) = &options.container {
        // Keep the selected video's container when audio is merged. This changes
        // muxing only; the selected stream and its codec are not re-encoded.
        command.args(["--merge-output-format", container]);
    }
    command
        .arg("--ffmpeg-location")
        .arg(&ffmpeg.path)
        .arg("--format")
        .arg(format)
        .arg("--paths")
        .arg(&options.directory)
        // Different quality/codec choices must not reuse another format's file.
        .args(["--output", "%(title)s [%(id)s] [%(format_id)s].%(ext)s"]);
    if let Some(directory) = temporary_directory {
        if !directory.is_absolute() {
            return Err(error(
                "invalidDownloadDirectory",
                "Temporary directory must be absolute",
            ));
        }
        command
            .arg("--paths")
            .arg(format!("temp:{}", directory.display()));
    }
    command.arg("--").arg(url);
    Ok(command)
}

fn download_format_selector(options: &DownloadCommandOptions, source: &str) -> String {
    let exact_filter = format!("[format_id=\"{}\"]", options.format_id);
    let select = |filter: &str| format!("bestvideo{filter}+bestaudio/best*{filter}");
    let exact = select(&exact_filter);
    let douyin = url::Url::parse(source).ok().is_some_and(|url| {
        url.host_str()
            .is_some_and(|host| host == "douyin.com" || host.ends_with(".douyin.com"))
    });
    let Some(selected) = options
        .selected_format
        .as_ref()
        .filter(|format| format.format_id == options.format_id)
    else {
        return exact;
    };
    if !douyin {
        return exact;
    }
    // Douyin returns several mirror URLs for one codec/resolution/bitrate family.
    // yt-dlp's duplicate suffix is request-specific, so retain the exact ID first
    // and accept only other mirrors with the same known quality and frame rate.
    let family = options
        .format_id
        .rsplit_once('-')
        .filter(|(_, suffix)| !suffix.is_empty() && suffix.bytes().all(|c| c.is_ascii_digit()))
        .map(|(family, _)| family)
        .unwrap_or(&options.format_id);
    let parts = family.split('_').collect::<Vec<_>>();
    if parts.len() != 3
        || !matches!(parts[0], "h264" | "bytevc1")
        || !parts[1]
        .strip_suffix('p')
        .is_some_and(|height| !height.is_empty() && height.bytes().all(|c| c.is_ascii_digit()))
        || parts[2].is_empty()
        || !parts[2].bytes().all(|c| c.is_ascii_digit())
    {
        return exact;
    }
    let Some(height) = selected.height.filter(|height| *height > 0) else {
        return exact;
    };
    let Some(extension) = selected
        .extension
        .as_deref()
        .filter(|ext| !ext.is_empty() && ext.bytes().all(|c| c.is_ascii_alphanumeric()))
    else {
        return exact;
    };
    let Some(fps) = selected.fps.filter(|fps| fps.is_finite() && *fps > 0.0) else {
        return exact;
    };
    let quality = format!("[height={height}][ext=\"{extension}\"][fps={fps}]");
    [
        format!("{exact_filter}{quality}"),
        format!("[format_id=\"{family}\"]{quality}"),
        format!("[format_id^=\"{family}-\"]{quality}"),
    ]
        .iter()
        .map(|filter| select(filter))
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
#[derive(Debug, Serialize)]
pub struct VideoCommand {
    pub(super) text: String,
    pub(super) shell: &'static str,
}

#[cfg(test)]
pub(super) fn render_command(command: &Command, windows: bool) -> Result<VideoCommand, VideoError> {
    let command = command.as_std();
    let arguments = std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(|argument| {
            let text = argument.to_str().ok_or_else(|| {
                error(
                    "commandUnavailable",
                    "Command contains non-Unicode arguments",
                )
            })?;
            let escaped = if windows {
                text.replace('\'', "''")
            } else {
                text.replace('\'', "'\"'\"'")
            };
            Ok(format!("'{escaped}'"))
        })
        .collect::<Result<Vec<_>, VideoError>>()?;
    Ok(VideoCommand {
        text: format!("{}{}", if windows { "& " } else { "" }, arguments.join(" ")),
        shell: if windows { "PowerShell" } else { "Shell" },
    })
}

#[cfg(test)]
pub(super) fn command_preview(
    settings: &RequiredToolSettings,
    store: &CookieStore,
    platform: CookiePlatform,
    input: &str,
    proxy: Option<&ProxySettings>,
) -> Result<VideoCommand, VideoError> {
    let url = normalize_link(input, platform)?;
    let has_cookie = !store
        .load(platform)
        .map_err(|e| error("cookieReadFailed", e.detail))?
        .is_empty();
    // The execution snapshot is removed after parsing. Use the maintained file path
    // for a reusable command; never expose Cookie contents or create a new snapshot.
    let path = store.path(platform);
    let command = parsing_command(settings, &url, has_cookie.then_some(path.as_path()), proxy)?;
    render_command(&command, cfg!(windows))
}

#[cfg(test)]
pub(super) fn download_command_preview(
    settings: &RequiredToolSettings,
    store: &CookieStore,
    platform: CookiePlatform,
    input: &str,
    options: &DownloadCommandOptions,
    proxy: Option<&ProxySettings>,
) -> Result<VideoCommand, VideoError> {
    let url = normalize_link(input, platform)?;
    let has_cookie = !options.cookie_fallback
        && !store
            .load(platform)
            .map_err(|e| error("cookieReadFailed", e.detail))?
            .is_empty();
    let path = store.path(platform);
    let command = download_command(
        settings,
        &url,
        has_cookie.then_some(path.as_path()),
        options,
        proxy,
    )?;
    render_command(&command, cfg!(windows))
}
