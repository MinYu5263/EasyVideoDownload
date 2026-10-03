use super::{
    commands::{download_command, DownloadCommandOptions},
    cookie_snapshot, error, normalize_link, VideoError,
};
use crate::{
    cookies::{CookiePlatform, CookieStore},
    required_tools::{process_tree, RequiredToolManager},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Mutex,
    time::Duration,
};
use tauri::{ipc::Channel, Manager};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader},
    process::Command,
    sync::watch,
};
mod progress;
use progress::ProgressTracker;

#[derive(Clone, Debug, Serialize)]
pub struct DownloadProgress {
    phase: &'static str,
    percent: Option<f64>,
    speed: Option<f64>,
    eta: Option<f64>,
}
impl DownloadProgress {
    fn phase(phase: &'static str) -> Self {
        Self {
            phase,
            percent: None,
            speed: None,
            eta: None,
        }
    }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadResult {
    path: String,
    already_downloaded: bool,
}

#[derive(Default)]
pub struct DownloadManager {
    active: Mutex<Option<(String, watch::Sender<bool>)>>,
}
struct ActiveDownload<'a>(&'a DownloadManager);
impl Drop for ActiveDownload<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.0.active.lock() {
            active.take();
        }
    }
}
impl DownloadManager {
    fn begin(&self, id: String) -> Result<(ActiveDownload<'_>, watch::Receiver<bool>), VideoError> {
        let mut active = self.active.lock().map_err(|e| error("bridgeFailed", e))?;
        if active.is_some() {
            return Err(error("downloadBusy", "A download is already running"));
        }
        let (sender, receiver) = watch::channel(false);
        *active = Some((id, sender));
        Ok((ActiveDownload(self), receiver))
    }
    fn cancel(&self, id: &str) -> Result<(), VideoError> {
        let active = self.active.lock().map_err(|e| error("bridgeFailed", e))?;
        if let Some((current, sender)) = active.as_ref() {
            if current == id {
                let _ = sender.send(true);
            }
        }
        Ok(())
    }
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
#[tauri::command]
pub fn cancel_video_download(
    request_id: String,
    downloads: tauri::State<'_, DownloadManager>,
) -> Result<(), VideoError> {
    downloads.cancel(&request_id)
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
        } else if !line.is_empty() {
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
    let lower = detail.to_ascii_lowercase();
    let detail = if lower.contains("cookie") || lower.contains("traceback") {
        "yt-dlp failed; check the platform Cookie file and tool version".to_owned()
    } else {
        detail.chars().take(4096).collect()
    };
    error(
        "downloadFailed",
        if detail.trim().is_empty() {
            "yt-dlp exited unsuccessfully".into()
        } else {
            detail
        },
    )
}
fn confirmed_file(path: &Path, directory: &Path) -> Result<DownloadResult, VideoError> {
    let file = std::fs::canonicalize(path).map_err(|e| error("downloadResultMissing", e))?;
    let directory =
        std::fs::canonicalize(directory).map_err(|e| error("downloadResultMissing", e))?;
    let metadata = std::fs::metadata(&file).map_err(|e| error("downloadResultMissing", e))?;
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
    })
}
async fn run_download(
    mut command: Command,
    directory: &Path,
    mut cancel: watch::Receiver<bool>,
    limit: Duration,
    notify: impl Fn(DownloadProgress) -> Result<(), VideoError> + Sync,
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
    let result = tokio::select! {
        biased;
        _ = async { if !*cancel.borrow() { let _ = cancel.changed().await; } } => Err(error("downloadCancelled", "")),
        result = tokio::time::timeout(limit, async {
            tokio::try_join!(async {child.wait().await.map_err(|e| error("readFailed", e))}, read_stream(stdout, &tracker, &notify), read_stream(stderr, &tracker, &notify))
        }) => match result { Ok(result) => result, Err(_) => Err(error("downloadTimeout", "Download exceeded six hours")) },
    };
    drop(tree);
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
pub async fn download_video(
    platform: CookiePlatform,
    input: String,
    options: DownloadCommandOptions,
    request_id: String,
    on_progress: Channel<DownloadProgress>,
    tools: tauri::State<'_, RequiredToolManager>,
    cookies: tauri::State<'_, CookieStore>,
    downloads: tauri::State<'_, DownloadManager>,
) -> Result<DownloadResult, VideoError> {
    let (_active, cancel) = downloads.begin(request_id)?;
    let notify = |update| {
        on_progress
            .send(update)
            .map_err(|e| error("bridgeFailed", e))
    };
    notify(DownloadProgress::phase("preparing"))?;
    let url = normalize_link(&input, platform)?;
    let settings = tools
        .settings_snapshot()
        .map_err(|e| error("toolSettingsFailed", e.detail))?;
    let store = cookies.inner().clone();
    let fallback = options.cookie_fallback;
    let copy = tauri::async_runtime::spawn_blocking(move || {
        if fallback {
            Ok(None)
        } else {
            cookie_snapshot(&store, platform)
        }
    })
    .await
    .map_err(|e| error("cookieReadFailed", e))??;
    let command = download_command(
        &settings,
        &url,
        copy.as_ref().map(|file| file.path()),
        &options,
    )?;
    if *cancel.borrow() {
        return Err(error("downloadCancelled", ""));
    }
    let directory = PathBuf::from(&options.directory);
    let output = directory.clone();
    tauri::async_runtime::spawn_blocking(move || std::fs::create_dir_all(output))
        .await
        .map_err(|e| error("downloadDirectoryFailed", e))?
        .map_err(|e| error("downloadDirectoryFailed", e))?;
    // Retain the Cookie snapshot until yt-dlp and FFmpeg have both stopped.
    run_download(
        command,
        &directory,
        cancel,
        Duration::from_secs(6 * 60 * 60),
        notify,
    )
    .await
}

#[cfg(test)]
mod tests;
