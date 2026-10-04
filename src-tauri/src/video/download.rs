use super::{
    commands::{download_command, DownloadCommandOptions},
    cookie_snapshot, error, normalize_link, VideoError,
};
use crate::{
    cookies::{CookiePlatform, CookieStore},
    database::Storage,
    required_tools::{
        process_tree, validate_program_files, RequiredToolId, RequiredToolManager,
        RequiredToolSettings,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Mutex,
    time::Duration,
};
use tauri::Manager;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader},
    process::Command,
    sync::watch,
};
mod failure;
pub(crate) mod history;
mod progress;
use history::DownloadRecordSession;
use progress::ProgressTracker;

#[derive(Clone, Debug, Serialize)]
pub struct DownloadProgress {
    phase: &'static str,
    percent: Option<f64>,
    speed: Option<f64>,
    eta: Option<f64>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadResult {
    path: String,
    already_downloaded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    storage_error: Option<crate::database::StorageError>,
}

pub(super) mod task_snapshot;
pub(crate) mod tasks;
pub use tasks::DownloadManager;

async fn prepare_download_directory(directory: PathBuf) -> Result<(), VideoError> {
    tauri::async_runtime::spawn_blocking(move || -> std::io::Result<()> {
        std::fs::create_dir_all(&directory)?;
        let probe = tempfile::Builder::new()
            .prefix(".easyvideo-write-check-")
            .tempfile_in(&directory)?;
        probe.close()?;
        Ok(())
    })
        .await
        .map_err(|e| error("downloadDirectoryFailed", e))?
        .map_err(|e| error("downloadDirectoryFailed", e))
}

fn default_directories(
    video: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Result<BTreeMap<&'static str, String>, VideoError> {
    let base = video
        .or_else(|| {
            home.map(|path| {
                path.join(if cfg!(target_os = "macos") {
                    "Movies"
                } else {
                    "Videos"
                })
            })
        })
        .filter(|path| path.is_absolute())
        .ok_or_else(|| {
            error(
                "defaultDirectoryFailed",
                "The system video and home directories are unavailable",
            )
        })?
        .join("EasyVideoDownload");
    Ok(["douyin", "bilibili", "youtube"]
        .into_iter()
        .map(|platform| (platform, base.join(platform).to_string_lossy().into_owned()))
        .collect())
}
#[tauri::command]
pub fn get_default_download_directories(
    app: tauri::AppHandle,
) -> Result<BTreeMap<&'static str, String>, VideoError> {
    default_directories(app.path().video_dir().ok(), app.path().home_dir().ok())
}
struct StreamOutput {
    path: Option<PathBuf>,
    already_downloaded: bool,
    diagnostic: String,
}
#[derive(Deserialize)]
struct FileReport {
    filepath: PathBuf,
    #[serde(rename = "__real_download")]
    downloaded: Option<bool>,
}
async fn read_stream(
    stream: impl AsyncRead + Unpin,
    tracker: &Mutex<ProgressTracker>,
    notify: &(impl Fn(DownloadProgress) -> Result<(), VideoError> + Sync),
    history: Option<&DownloadRecordSession>,
) -> Result<StreamOutput, VideoError> {
    let mut reader = BufReader::new(stream);
    let mut output = StreamOutput {
        path: None,
        already_downloaded: false,
        diagnostic: String::new(),
    };
    loop {
        let mut bytes = Vec::new();
        let count = (&mut reader)
            .take(65_537)
            .read_until(b'\n', &mut bytes)
            .await
            .map_err(|e| error("readFailed", e))?;
        if count == 0 {
            break;
        }
        if count > 65_536 {
            return Err(error(
                "outputTooLarge",
                "yt-dlp output line exceeded its limit",
            ));
        }
        let line = String::from_utf8_lossy(&bytes);
        let line = line.trim();
        if let Some(history) = history {
            history.observe(line).await;
        }
        if let Some(path) = line.strip_prefix("__EVD_FILE__") {
            let report: FileReport =
                serde_json::from_str(path).map_err(|e| error("downloadResultMissing", e))?;
            let path = report.filepath;
            if output
                .path
                .as_ref()
                .is_some_and(|previous| previous != &path)
            {
                return Err(error(
                    "unsupportedVideo",
                    "Multiple output videos are not supported",
                ));
            }
            output.path = Some(path);
            // This flag covers the final output. A skipped audio/video fragment alone
            // does not mean the entire video already existed before this attempt.
            output.already_downloaded = report.downloaded == Some(false);
            notify(
                tracker
                    .lock()
                    .map_err(|e| error("readFailed", e))?
                    .finalizing(),
            )?;
            continue;
        }
        let update = tracker
            .lock()
            .map_err(|e| error("readFailed", e))?
            .update(line);
        if let Some(update) = update {
            notify(update)?;
        } else if !line.is_empty() && !line.starts_with("[debug]") {
            output.diagnostic.push_str(line);
            output.diagnostic.push('\n');
            // Keep a bounded tail while continuously draining long-running downloads.
            if output.diagnostic.len() > 8192 {
                let mut start = output.diagnostic.len() - 4096;
                while !output.diagnostic.is_char_boundary(start) {
                    start += 1;
                }
                output.diagnostic.drain(..start);
            }
        }
    }
    Ok(output)
}
fn download_failure(detail: &str) -> VideoError {
    let kind = failure::failure_kind("downloadFailed", detail, "unknown");
    let detail = failure::sanitize_diagnostic(detail);
    let mut failure = error(
        "downloadFailed",
        if detail.trim().is_empty() {
            "yt-dlp exited unsuccessfully".into()
        } else {
            detail
        },
    );
    failure.failure_kind = (kind != "unknown").then_some(kind);
    failure
}

fn ensure_download_tools(
    settings: &RequiredToolSettings,
    platform: CookiePlatform,
) -> Result<(), VideoError> {
    for id in [
        RequiredToolId::Ytdlp,
        RequiredToolId::Ffmpeg,
        RequiredToolId::Deno,
    ] {
        // Deno is only needed for YouTube's JavaScript challenges. Keep the
        // existing optional runtime behavior when no Deno has been configured.
        if id == RequiredToolId::Deno
            && (!matches!(platform, CookiePlatform::Youtube) || !settings.tools.contains_key(&id))
        {
            continue;
        }
        let missing_code = match id {
            RequiredToolId::Ytdlp => "toolMissing",
            RequiredToolId::Ffmpeg => "ffmpegMissing",
            RequiredToolId::Deno => "denoMissing",
        };
        let config = settings
            .tools
            .get(&id)
            .ok_or_else(|| error(missing_code, ""))?;
        validate_program_files(id, config).map_err(|failure| {
            error(
                if failure.code == "notFound" {
                    missing_code
                } else {
                    "toolSettingsFailed"
                },
                format!("{}\n{}", failure.program, failure.detail),
            )
        })?;
    }
    Ok(())
}

async fn check_download_tool_files(
    settings: &RequiredToolSettings,
    platform: CookiePlatform,
) -> Result<(), VideoError> {
    let settings = settings.clone();
    tauri::async_runtime::spawn_blocking(move || ensure_download_tools(&settings, platform))
        .await
        .map_err(|failure| error("toolSettingsFailed", failure))?
}

fn output_access_failure(failure: std::io::Error) -> VideoError {
    error(
        if failure::file_is_occupied(&failure) {
            "historyFileOccupied"
        } else {
            "downloadResultMissing"
        },
        failure,
    )
}

fn confirmed_file(path: &Path, directory: &Path) -> Result<DownloadResult, VideoError> {
    let file = std::fs::canonicalize(path).map_err(output_access_failure)?;
    let directory = std::fs::canonicalize(directory).map_err(output_access_failure)?;
    let metadata = std::fs::metadata(&file).map_err(output_access_failure)?;
    if !file.starts_with(directory) || !metadata.is_file() || metadata.len() == 0 {
        return Err(error(
            "downloadResultMissing",
            "No non-empty output video exists in the chosen directory",
        ));
    }
    // Strip Windows' extended path prefix from the path displayed to the user.
    let display = file.to_string_lossy();
    let path = if let Some(unc) = display.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{unc}")
    } else {
        display
            .strip_prefix("\\\\?\\")
            .unwrap_or(&display)
            .to_owned()
    };
    Ok(DownloadResult {
        path,
        already_downloaded: false,
        storage_error: None,
    })
}
#[cfg(test)]
async fn run_download(
    command: Command,
    directory: &Path,
    cancel: watch::Receiver<bool>,
    limit: Duration,
    notify: impl Fn(DownloadProgress) -> Result<(), VideoError> + Sync,
) -> Result<DownloadResult, VideoError> {
    run_download_with_history(command, directory, cancel, limit, notify, None).await
}
async fn run_download_with_history(
    mut command: Command,
    directory: &Path,
    mut cancel: watch::Receiver<bool>,
    limit: Duration,
    notify: impl Fn(DownloadProgress) -> Result<(), VideoError> + Sync,
    history: Option<&DownloadRecordSession>,
) -> Result<DownloadResult, VideoError> {
    if *cancel.borrow() {
        return Err(error("downloadCancelled", ""));
    }
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
    let tracker = Mutex::new(ProgressTracker::default());
    let result = {
        let mut processing = Box::pin(async {
            tokio::try_join!(
                async { child.wait().await.map_err(|e| error("readFailed", e)) },
                read_stream(stdout, &tracker, &notify, history),
                read_stream(stderr, &tracker, &notify, history)
            )
        });
        let result = tokio::select! {
            biased;
            _ = async { if !*cancel.borrow() { let _ = cancel.changed().await; } } => Err(error("downloadCancelled", "")),
            result = tokio::time::timeout(limit, &mut processing) => match result { Ok(result) => Ok(result), Err(_) => Err(error("downloadTimeout", "Download exceeded six hours")) },
        };
        // Kill the process tree, then consume its buffered start evidence before settlement.
        drop(tree);
        match result {
            Ok(result) => result,
            Err(failure) => {
                let _ = tokio::time::timeout(Duration::from_secs(10), &mut processing).await;
                Err(failure)
            }
        }
    };
    let (status, stdout, stderr) = match result {
        Ok(output) => output,
        Err(failure) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Err(failure);
        }
    };
    if !status.success() {
        return Err(download_failure(&format!(
            "{}{}",
            stdout.diagnostic, stderr.diagnostic
        )));
    }
    let output = if stdout.path.is_some() {
        stdout
    } else {
        stderr
    };
    let path = output.path.ok_or_else(|| {
        error(
            "downloadResultMissing",
            "yt-dlp did not report a final output path",
        )
    })?;
    let directory = directory.to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        let mut result = confirmed_file(&path, &directory)?;
        result.already_downloaded = output.already_downloaded;
        Ok(result)
    })
    .await
    .map_err(|e| error("downloadResultMissing", e))?
}

#[tauri::command]
pub fn create_download_request_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests;
