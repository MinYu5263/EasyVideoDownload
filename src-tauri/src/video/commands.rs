use super::{error, normalize_link, VideoError};
use crate::cookies::{CookiePlatform, CookieStore};
use crate::required_tools::{RequiredToolId, RequiredToolSettings};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::process::Command;

fn configured_command(
    settings: &RequiredToolSettings,
    cookie: Option<&Path>,
    purpose: &[&str],
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
) -> Result<Command, VideoError> {
    let mut command = configured_command(
        settings,
        cookie,
        &["--dump-single-json", "--simulate", "--no-progress"],
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
}

pub(super) fn download_command(
    settings: &RequiredToolSettings,
    url: &str,
    cookie: Option<&Path>,
    options: &DownloadCommandOptions,
) -> Result<Command, VideoError> {
    if !Path::new(&options.directory).is_absolute() {
        return Err(error(
            "invalidDownloadDirectory",
            "Choose an absolute output directory",
        ));
    }
    if options.format_id.is_empty()
        || !options
            .format_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_.-:".contains(c))
    {
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
            "before_dl:__EVD_PLAN__{\"formats\":%(requested_formats.:.{format_id,filesize,filesize_approx}|[])j,\"formatId\":%(format_id)j,\"size\":%(filesize,filesize_approx|0)j}",
            "--print",
            "after_move:__EVD_FILE__%(.{filepath,__real_download})j",
        ],
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
    let filters = format!("[format_id=\"{}\"]", options.format_id);
    let format = format!("bestvideo{filters}+bestaudio/best*{filters}");
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
    command.arg("--").arg(url);
    Ok(command)
}

#[derive(Debug, Serialize)]
pub struct VideoCommand {
    pub(super) text: String,
    pub(super) shell: &'static str,
}

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

pub(super) fn command_preview(
    settings: &RequiredToolSettings,
    store: &CookieStore,
    platform: CookiePlatform,
    input: &str,
) -> Result<VideoCommand, VideoError> {
    let url = normalize_link(input, platform)?;
    let has_cookie = !store
        .load(platform)
        .map_err(|e| error("cookieReadFailed", e.detail))?
        .is_empty();
    // The execution snapshot is removed after parsing. Use the maintained file path
    // for a reusable command; never expose Cookie contents or create a new snapshot.
    let path = store.path(platform);
    let command = parsing_command(settings, &url, has_cookie.then_some(path.as_path()))?;
    render_command(&command, cfg!(windows))
}

pub(super) fn download_command_preview(
    settings: &RequiredToolSettings,
    store: &CookieStore,
    platform: CookiePlatform,
    input: &str,
    options: &DownloadCommandOptions,
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
    )?;
    render_command(&command, cfg!(windows))
}
