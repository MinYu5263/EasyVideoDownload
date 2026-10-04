use super::*;
use crate::proxy;
use sha2::{Digest, Sha256};
use std::io::Write;
use tauri::ipc::Channel;
use tokio::sync::watch;
mod downloads;

const MAX_BINARY: usize = 128 * 1024 * 1024;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigureProgress {
    phase: &'static str,
    downloaded: u64,
    total: Option<u64>,
}

fn asset(os: &str, arch: &str) -> Result<&'static str, RequiredToolError> {
    match (os, arch) {
        ("windows", "x86_64") => Ok("yt-dlp.exe"),
        ("windows", "x86") => Ok("yt-dlp_x86.exe"),
        ("windows", "aarch64") => Ok("yt-dlp_arm64.exe"),
        ("macos", "x86_64" | "aarch64") => Ok("yt-dlp_macos.zip"),
        _ => Err(error("automaticUnsupported", "yt-dlp", "")),
    }
}

pub(super) fn supported(id: RequiredToolId) -> bool {
    match id {
        RequiredToolId::Ytdlp => asset(std::env::consts::OS, std::env::consts::ARCH).is_ok(),
        _ => {
            matches!(std::env::consts::OS, "windows" | "macos")
                && matches!(std::env::consts::ARCH, "x86_64" | "aarch64")
        }
    }
}

fn tool_directory(id: RequiredToolId, storage: &Storage) -> Result<PathBuf, RequiredToolError> {
    Ok(storage
        .prepare_data_directory()
        .map_err(storage_error)?
        .join("tools")
        .join(id.names()[0]))
}

fn executable_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.into()
    }
}

pub(super) async fn detect_managed(
    id: RequiredToolId,
    storage: &Storage,
) -> Result<RequiredToolConfig, RequiredToolError> {
    if !supported(id) {
        return Err(error("automaticUnsupported", id.names()[0], ""));
    }
    let directory = tool_directory(id, storage)?;
    if !directory
        .try_exists()
        .map_err(|e| error("readFailed", id.names()[0], e))?
    {
        return Err(error("notFound", id.names()[0], directory.display()));
    }
    detect_path_with_cancel(id, &directory, None).await
}

async fn detect_path_with_cancel(
    id: RequiredToolId,
    directory: &Path,
    cancellation: Option<watch::Receiver<bool>>,
) -> Result<RequiredToolConfig, RequiredToolError> {
    let mut config = detect_with_cancel(
        &RequiredToolRequest {
            tool_id: id,
            source: RequiredToolSource::Manual,
            manual_path: if id == RequiredToolId::Ffmpeg {
                directory.to_path_buf()
            } else {
                directory.join(executable_name(id.names()[0]))
            }
                .to_string_lossy()
                .into_owned(),
        },
        cancellation,
    )
        .await?;
    config.source = RequiredToolSource::Automatic;
    config.manual_path.clear();
    Ok(config)
}

fn checksum(text: &str, filename: &str) -> Result<Vec<u8>, RequiredToolError> {
    let mut matches = text.lines().filter_map(|line| {
        let mut fields = line.split_whitespace();
        let hash = fields.next()?;
        let name = fields.next()?.trim_start_matches('*');
        (name == filename && fields.next().is_none()).then_some(hash)
    });
    let hash = matches
        .next()
        .ok_or_else(|| error("downloadInvalid", "yt-dlp", "missing checksum"))?;
    if matches.next().is_some()
        || hash.len() != 64
        || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(error("downloadInvalid", "yt-dlp", "invalid checksum"));
    }
    Ok((0..64)
        .step_by(2)
        .map(|i| u8::from_str_radix(&hash[i..i + 2], 16).unwrap())
        .collect())
}

async fn read_response(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, RequiredToolError> {
    let mut bytes = Vec::new();
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(error("downloadInvalid", "yt-dlp", "response too large"));
    }
    while let Some(chunk) = response.chunk().await.map_err(download_error)? {
        if bytes.len() + chunk.len() > limit {
            return Err(error("downloadInvalid", "yt-dlp", "response too large"));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn download_error(failure: reqwest::Error) -> RequiredToolError {
    // Do not expose proxy credentials or signed asset URLs in UI errors.
    error(
        if failure.is_timeout() {
            "configureTimeout"
        } else {
            "configureDownloadFailed"
        },
        "",
        failure
            .status()
            .map(|status| format!("HTTP {}", status.as_u16()))
            .unwrap_or_default(),
    )
}

async fn get(client: &reqwest::Client, url: &str) -> Result<reqwest::Response, RequiredToolError> {
    client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(download_error)
}

async fn latest_release_tag(
    client: &reqwest::Client,
    repository: &str,
) -> Result<String, RequiredToolError> {
    let response = get(
        client,
        &format!("https://github.com/{repository}/releases/latest"),
    )
        .await?;
    let prefix = format!("/{repository}/releases/tag/");
    let url = response.url();
    if url.host_str() != Some("github.com") {
        return Err(error("downloadInvalid", "", "unexpected release source"));
    }
    url.path()
        .strip_prefix(&prefix)
        .filter(|tag| !tag.is_empty() && !tag.contains('/'))
        .map(str::to_owned)
        .ok_or_else(|| error("downloadInvalid", "", "invalid release redirect"))
}

async fn download_proxy(
    settings: Option<proxy::ProxySettings>,
    target: &str,
) -> Option<proxy::ProxySettings> {
    let settings = settings?.normalized().ok()?;
    // Probe the selected official source quietly; do not alter saved settings.
    let proxy = reqwest::Proxy::all(settings.url()).ok()?.no_proxy(None);
    let probe = reqwest::Client::builder()
        .no_proxy()
        .proxy(proxy)
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("EasyVideoDownload/0.1")
        .build()
        .ok()?;
    let response = probe.head(target).send().await.ok()?;
    (response.status().is_success() || response.status().is_redirection()).then_some(settings)
}

async fn client(
    storage: &Storage,
    id: RequiredToolId,
) -> Result<reqwest::Client, RequiredToolError> {
    let target = match id {
        RequiredToolId::Ytdlp => "https://github.com/yt-dlp/yt-dlp/releases/latest",
        RequiredToolId::Deno => "https://github.com/denoland/deno/releases/latest",
        RequiredToolId::Ffmpeg if cfg!(windows) => {
            "https://github.com/GyanD/codexffmpeg/releases/latest"
        }
        RequiredToolId::Ffmpeg => "https://evermeet.cx/ffmpeg/info/ffmpeg/release",
    };
    let saved = proxy::saved_proxy(storage).await.map_err(storage_error)?;
    let available = download_proxy(saved, target).await;
    // With no usable app proxy, preserve the process environment and native
    // system proxy settings. An explicit proxy below takes precedence.
    let mut builder = reqwest::Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .user_agent("EasyVideoDownload/0.1")
        .redirect(reqwest::redirect::Policy::limited(8));
    if let Some(settings) = available {
        builder = builder.proxy(
            reqwest::Proxy::all(settings.url())
                .map_err(|_| error("configureDownloadFailed", "yt-dlp", "invalid proxy"))?
                .no_proxy(None),
        );
    }
    builder.build().map_err(download_error)
}

async fn download_ytdlp(
    client: &reqwest::Client,
    filename: &str,
    destination: &Path,
    progress: &impl Fn(ConfigureProgress),
) -> Result<(), RequiredToolError> {
    progress(ConfigureProgress {
        phase: "preparing",
        downloaded: 0,
        total: None,
    });
    let tag = latest_release_tag(client, "yt-dlp/yt-dlp").await?;
    parse_version("yt-dlp", &tag)?;
    let base = format!("https://github.com/yt-dlp/yt-dlp/releases/download/{}", tag);
    let sums = read_response(
        get(client, &format!("{base}/SHA2-256SUMS")).await?,
        64 * 1024,
    )
        .await?;
    let expected = checksum(
        std::str::from_utf8(&sums)
            .map_err(|_| error("downloadInvalid", "yt-dlp", "invalid checksum text"))?,
        filename,
    )?;
    let mut response = get(client, &format!("{base}/{filename}")).await?;
    let total = response.content_length();
    if total.is_some_and(|size| size > MAX_BINARY as u64) {
        return Err(error("downloadInvalid", "yt-dlp", "binary too large"));
    }
    let mut file = std::fs::File::create(destination)
        .map_err(|e| error("configureWriteFailed", "yt-dlp", e))?;
    let mut hasher = Sha256::new();
    let mut downloaded = 0;
    let mut last = std::time::Instant::now();
    progress(ConfigureProgress {
        phase: "downloading",
        downloaded,
        total,
    });
    while let Some(chunk) = response.chunk().await.map_err(download_error)? {
        downloaded += chunk.len() as u64;
        if downloaded > MAX_BINARY as u64 {
            return Err(error("downloadInvalid", "yt-dlp", "binary too large"));
        }
        file.write_all(&chunk)
            .map_err(|e| error("configureWriteFailed", "yt-dlp", e))?;
        hasher.update(&chunk);
        if last.elapsed() >= Duration::from_millis(100) {
            progress(ConfigureProgress {
                phase: "downloading",
                downloaded,
                total,
            });
            last = std::time::Instant::now();
        }
    }
    file.sync_all()
        .map_err(|e| error("configureWriteFailed", "yt-dlp", e))?;
    drop(file);
    progress(ConfigureProgress {
        phase: "verifying",
        downloaded,
        total,
    });
    if downloaded == 0 || hasher.finalize()[..] != expected[..] {
        return Err(error("downloadInvalid", "yt-dlp", "checksum mismatch"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(destination, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| error("configureWriteFailed", "yt-dlp", e))?;
    }
    Ok(())
}

// The temporary directory owns the exact staging and backup paths. A failed save
// restores the previous executable before removing this installation's files.
struct Installation {
    directory: Option<tempfile::TempDir>,
    target: PathBuf,
    published: bool,
    had_previous: bool,
}

impl Installation {
    fn new(target: PathBuf) -> Result<Self, RequiredToolError> {
        let parent = target
            .parent()
            .ok_or_else(|| error("configureWriteFailed", "yt-dlp", "missing directory"))?;
        std::fs::create_dir_all(parent).map_err(|e| error("configureWriteFailed", "yt-dlp", e))?;
        let directory = tempfile::Builder::new()
            .prefix(".configure-")
            .tempdir_in(parent)
            .map_err(|e| error("configureWriteFailed", "yt-dlp", e))?;
        std::fs::create_dir(directory.path().join("new"))
            .map_err(|e| error("configureWriteFailed", "", e))?;
        Ok(Self {
            directory: Some(directory),
            target,
            published: false,
            had_previous: false,
        })
    }
    fn staged(&self) -> PathBuf {
        self.directory.as_ref().unwrap().path().join("new")
    }
    fn backup(&self) -> PathBuf {
        self.directory.as_ref().unwrap().path().join("previous")
    }
    fn publish(&mut self) -> Result<(), RequiredToolError> {
        if self
            .target
            .try_exists()
            .map_err(|e| error("configureWriteFailed", "yt-dlp", e))?
        {
            std::fs::rename(&self.target, self.backup())
                .map_err(|e| error("configureWriteFailed", "yt-dlp", e))?;
            self.had_previous = true;
        }
        std::fs::rename(self.staged(), &self.target)
            .map_err(|e| error("configureWriteFailed", "yt-dlp", e))?;
        self.published = true;
        Ok(())
    }
    fn rollback(&mut self) -> std::io::Result<()> {
        if self.published {
            std::fs::remove_dir_all(&self.target)?;
            self.published = false;
        }
        if self.had_previous {
            std::fs::rename(self.backup(), &self.target)?;
            self.had_previous = false;
        }
        Ok(())
    }
    fn finish(mut self, committed: bool) -> Result<(), RequiredToolError> {
        if committed {
            self.published = false;
            self.had_previous = false;
        } else {
            self.rollback()
                .map_err(|e| error("configureCleanupFailed", "yt-dlp", e))?;
        }
        if let Some(directory) = self.directory.take() {
            let path = directory.path().to_path_buf();
            directory.close().map_err(|e| {
                error(
                    "configureCleanupFailed",
                    "yt-dlp",
                    format!("{}: {e}", path.display()),
                )
            })?;
        }
        Ok(())
    }
}

impl Drop for Installation {
    fn drop(&mut self) {
        if self.rollback().is_err() {
            // Retain the backup for recovery if the filesystem prevents rollback.
            if let Some(directory) = self.directory.take() {
                let _ = directory.keep();
            }
        }
    }
}

pub struct ConfigureManager {
    cancel: [StdMutex<Option<watch::Sender<bool>>>; 3],
    lifecycle: StdMutex<(bool, usize)>,
}
impl Default for ConfigureManager {
    fn default() -> Self {
        Self {
            cancel: std::array::from_fn(|_| StdMutex::new(None)),
            lifecycle: StdMutex::new((false, 0)),
        }
    }
}
impl ConfigureManager {
    fn begin_job(&self) -> Result<ConfigureJob<'_>, RequiredToolError> {
        let mut lifecycle = self
            .lifecycle
            .lock()
            .map_err(|e| error("bridgeFailed", "yt-dlp", e))?;
        if lifecycle.0 {
            return Err(error(
                "configureCancelled",
                "yt-dlp",
                "The application is exiting",
            ));
        }
        lifecycle.1 += 1;
        Ok(ConfigureJob(self))
    }
    pub(crate) fn begin_exit(&self) -> Result<(), String> {
        self.lifecycle.lock().map_err(|e| e.to_string())?.0 = true;
        for cancel in &self.cancel {
            if let Some(sender) = cancel.lock().map_err(|e| e.to_string())?.as_ref() {
                let _ = sender.send(true);
            }
        }
        Ok(())
    }
    pub(crate) fn is_active(&self) -> Result<bool, String> {
        Ok(self.lifecycle.lock().map_err(|e| e.to_string())?.1 != 0)
    }
    pub(crate) fn abort_exit(&self) {
        if let Ok(mut lifecycle) = self.lifecycle.lock() {
            lifecycle.0 = false;
        }
    }
    fn cancel(&self, id: RequiredToolId) -> Result<bool, RequiredToolError> {
        let active = self.cancel[id.index()]
            .lock()
            .map_err(|e| error("bridgeFailed", "yt-dlp", e))?;
        Ok(active
            .as_ref()
            .is_some_and(|sender| sender.send(true).is_ok()))
    }
    fn begin_commit(
        &self,
        id: RequiredToolId,
        receiver: &watch::Receiver<bool>,
    ) -> Result<(), RequiredToolError> {
        let mut active = self.cancel[id.index()]
            .lock()
            .map_err(|e| error("bridgeFailed", "yt-dlp", e))?;
        if *receiver.borrow() {
            return Err(error("configureCancelled", "yt-dlp", ""));
        }
        active.take();
        Ok(())
    }
}
struct ConfigureJob<'a>(&'a ConfigureManager);
impl Drop for ConfigureJob<'_> {
    fn drop(&mut self) {
        if let Ok(mut lifecycle) = self.0.lifecycle.lock() {
            lifecycle.1 -= 1;
        }
    }
}
struct ActiveConfigure<'a>(&'a ConfigureManager, RequiredToolId);
impl Drop for ActiveConfigure<'_> {
    fn drop(&mut self) {
        if let Ok(mut cancel) = self.0.cancel[self.1.index()].lock() {
            cancel.take();
        }
    }
}

async fn configure(
    id: RequiredToolId,
    state: &RequiredToolManager,
    cancellation: &ConfigureManager,
    progress: impl Fn(ConfigureProgress),
) -> Result<CheckResult, RequiredToolError> {
    let _job = cancellation.begin_job()?;
    if !supported(id) {
        return Err(error("automaticUnsupported", id.names()[0], ""));
    }
    let _lock = state.checks[id.index()]
        .try_lock()
        .map_err(|_| error("busy", "yt-dlp", ""))?;
    state.settings_snapshot()?;
    let request = RequiredToolRequest {
        tool_id: id,
        source: RequiredToolSource::Automatic,
        manual_path: String::new(),
    };
    let (sender, mut receiver) = watch::channel(false);
    {
        let mut cancel = cancellation.cancel[id.index()]
            .lock()
            .map_err(|e| error("bridgeFailed", "yt-dlp", e))?;
        if cancel.is_some() {
            return Err(error("busy", "yt-dlp", ""));
        }
        if cancellation
            .lifecycle
            .lock()
            .map_err(|e| error("bridgeFailed", "yt-dlp", e))?
            .0
        {
            return Err(error(
                "configureCancelled",
                "yt-dlp",
                "The application is exiting",
            ));
        }
        *cancel = Some(sender);
    }
    let _active = ActiveConfigure(cancellation, id);
    progress(ConfigureProgress {
        phase: "checking",
        downloaded: 0,
        total: None,
    });
    // Recheck installed copies without downloading on a transient timeout.
    // Permanently invalid copies can still be repaired by automatic setup.
    let target = tool_directory(id, &state.storage)?;
    if *receiver.borrow() {
        return apply_result(state, &request, Err(error("configureCancelled", "", ""))).await;
    }
    if target.try_exists().map_err(|e| error("readFailed", "", e))?
        && downloads::reusable_layout(id, &target)
    {
        match detect_path_with_cancel(id, &target, Some(receiver.clone())).await {
            Ok(existing) => {
                cancellation.begin_commit(id, &receiver)?;
                return apply_result(state, &request, Ok(existing)).await;
            }
            Err(failure) if matches!(failure.code.as_str(), "timeout" | "configureCancelled" | "readFailed") => {
                return apply_result(state, &request, Err(failure)).await;
            }
            Err(_) => {},
        }
    }
    let mut installation = Installation::new(tool_directory(id, &state.storage)?)?;
    // Only complete, integrity-checked downloads enter this retry cache. Keep
    // them separate from active tools until native execution checks succeed.
    let pending = installation.target.with_extension("pending");
    // Retire only an obsolete application-owned pending layout. The installed
    // copy remains untouched until the replacement passes native validation.
    if pending.try_exists().map_err(|e| error("readFailed", "", e))?
        && !downloads::reusable_layout(id, &pending)
    {
        std::fs::rename(&pending, installation.directory.as_ref().unwrap().path().join("obsolete-pending"))
            .map_err(|e| error("configureWriteFailed", "", e))?;
    }
    let downloaded = if pending.try_exists().map_err(|e| error("readFailed", "", e))? {
        Ok(())
    } else {
        progress(ConfigureProgress {
            phase: "preparing",
            downloaded: 0,
            total: None,
        });
        let download_cancel = receiver.clone();
        let result = tokio::select! {
            biased;
            _ = receiver.changed() => Err(error("configureCancelled", "", "")),
            result = async {
                let http = client(&state.storage, id).await?;
                downloads::download_tool(id, &http, &installation.staged(), &progress, &download_cancel).await
            } => result,
        };
        if result.is_ok() {
            std::fs::rename(installation.staged(), &pending)
                .map_err(|e| error("configureWriteFailed", "", e))?;
        }
        result
    };
    let candidate = match downloaded {
        Ok(()) => {
            progress(ConfigureProgress {
                phase: "checking",
                downloaded: 0,
                total: None,
            });
            // Keep the path stable across retries and restarts. The collector kills
            // and waits before the verified directory can be moved on Windows.
            detect_path_with_cancel(id, &pending, Some(receiver.clone())).await
        }
        Err(failure) => Err(failure),
    };
    let mut config = match candidate {
        Ok(config) => config,
        Err(failure) => {
            installation.finish(false)?;
            return apply_result(state, &request, Err(failure)).await;
        }
    };
    if let Err(failure) = cancellation.begin_commit(id, &receiver) {
        installation.finish(false)?;
        return apply_result(state, &request, Err(failure)).await;
    }
    progress(ConfigureProgress {
        phase: "saving",
        downloaded: 0,
        total: None,
    });
    let _publication = state.usage.publication(&installation.target)?;
    if installation.staged().exists() {
        std::fs::remove_dir(installation.staged())
            .map_err(|e| error("configureWriteFailed", "", e))?;
    }
    std::fs::rename(&pending, installation.staged())
        .map_err(|e| error("configureWriteFailed", "", e))?;
    installation.publish()?;
    for program in &mut config.programs {
        program.path = installation.target.join(executable_name(&program.name));
    }
    let mut result = apply_result(state, &request, Ok(config)).await?;
    if let Err(failure) = installation.finish(result.error.is_none()) {
        result.error = Some(failure);
    }
    Ok(result)
}

#[tauri::command]
pub async fn configure_required_tool(
    tool_id: RequiredToolId,
    state: tauri::State<'_, RequiredToolManager>,
    cancellation: tauri::State<'_, ConfigureManager>,
    on_progress: Channel<ConfigureProgress>,
) -> Result<CheckResult, RequiredToolError> {
    let mut result = configure(tool_id, &state, &cancellation, |progress| {
        let _ = on_progress.send(progress);
    })
        .await
        .map_err(|mut e| {
            if e.program.is_empty() || e.program == "yt-dlp" {
                e.program = tool_id.names()[0].into();
            }
            e
        })?;
    if let Some(e) = &mut result.error {
        if e.program.is_empty() || e.program == "yt-dlp" {
            e.program = tool_id.names()[0].into();
        }
    }
    Ok(result)
}

#[tauri::command]
pub fn cancel_tool_configuration(
    tool_id: RequiredToolId,
    state: tauri::State<'_, ConfigureManager>,
) -> Result<bool, RequiredToolError> {
    state.cancel(tool_id)
}

#[cfg(test)]
mod tests {
    #[test]
    fn exit_tracks_tool_setup_beyond_cancellation_registration_and_blocks_new_jobs() {
        let manager = super::ConfigureManager::default();
        let job = manager.begin_job().unwrap();
        manager.begin_exit().unwrap();
        assert!(manager.begin_job().is_err());
        assert!(manager.is_active().unwrap());
        drop(job);
        assert!(!manager.is_active().unwrap());
        manager.abort_exit();
        assert!(manager.begin_job().is_ok());
    }
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn installed_tool_recheck_can_be_cancelled_without_replacement() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        eprintln!("cancel-recheck test directory: {}", root.path().display());
        let storage = Storage::new(&root.path().join("app.db"), &root.path().join("legacy.json"));
        let id = RequiredToolId::Ytdlp;
        if !supported(id) { return; }
        let target = tool_directory(id, &storage).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        let program = target.join("yt-dlp");
        let contents = b"#!/bin/sh\nsleep 60\n";
        std::fs::write(&program, contents).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        let state = RequiredToolManager::new(storage);
        let cancellation = ConfigureManager::default();
        let result = tokio::time::timeout(Duration::from_secs(3), configure(id, &state, &cancellation, |p| {
            assert_eq!(p.phase, "checking");
            assert!(cancellation.cancel(id).unwrap());
        })).await.unwrap().unwrap();
        assert_eq!(result.error.unwrap().code, "configureCancelled");
        assert_eq!(std::fs::read(program).unwrap(), contents);
        root.close().unwrap();
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn macos_ytdlp_check_allows_slow_startup() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        eprintln!("slow-start test directory: {}", root.path().display());
        let program = root.path().join("yt-dlp");
        std::fs::write(&program, "#!/bin/sh\ncase \"$2\" in\n--version) sleep 11; echo 2026.09.25;;\n*) echo 'Usage: yt-dlp [OPTIONS] URL'; echo '--ignore-config --extractor-args';;\nesac\n").unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        let result = detect_path_with_cancel(RequiredToolId::Ytdlp, root.path(), None).await;
        assert!(result.is_ok(), "{result:?}");
        root.close().unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn completed_download_is_rechecked_after_failure_and_restart_without_network() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        eprintln!("retry test directory: {}", root.path().display());
        let storage = Storage::new(&root.path().join("app.db"), &root.path().join("legacy.json"));
        let id = RequiredToolId::Ytdlp;
        if !supported(id) { return; }
        let target = tool_directory(id, &storage).unwrap();
        let pending = target.with_extension("pending");
        std::fs::create_dir_all(pending.join("_internal")).unwrap();
        std::fs::write(pending.join("_internal/Python"), b"runtime fixture").unwrap();
        std::fs::write(pending.join("_internal/base_library.zip"), b"stdlib fixture").unwrap();
        let program = pending.join("yt-dlp");
        std::fs::write(&program, "#!/bin/sh\nexit 7\n").unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        let state = RequiredToolManager::new(storage.clone());
        let result = tokio::time::timeout(Duration::from_secs(3), configure(id, &state, &ConfigureManager::default(), |p| {
            assert_ne!(p.phase, "preparing", "retry must not enter download preparation");
        })).await.unwrap().unwrap();
        assert_eq!(result.error.unwrap().code, "exitFailed");
        assert!(program.is_file());
        assert!(!target.exists());
        assert!(state.settings_snapshot().unwrap().tools.is_empty());
        std::fs::write(&program, "#!/bin/sh\ncase \"$2\" in\n--version) echo 2026.09.25;;\n*) echo 'Usage: yt-dlp [OPTIONS] URL'; echo '--ignore-config --extractor-args';;\nesac\n").unwrap();
        let restarted = RequiredToolManager::new(storage);
        let result = configure(id, &restarted, &ConfigureManager::default(), |p| {
            assert_ne!(p.phase, "preparing");
        }).await.unwrap();
        assert!(result.error.is_none(), "{:?}", result.error);
        assert!(result.active.unwrap().programs[0].path.is_file());
        assert!(!pending.exists());
        root.close().unwrap();
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn old_single_file_is_replaced_by_a_validated_pending_bundle() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        eprintln!("bundle migration test: {}", root.path().display());
        let storage = Storage::new(&root.path().join("app.db"), &root.path().join("legacy.json"));
        let id = RequiredToolId::Ytdlp;
        let target = tool_directory(id, &storage).unwrap();
        let pending = target.with_extension("pending");
        for (directory, version) in [(&target, "2024.01.01"), (&pending, "2026.09.25")] {
            std::fs::create_dir_all(directory).unwrap();
            let program = directory.join("yt-dlp");
            std::fs::write(&program, format!("#!/bin/sh\ncase \"$2\" in\n--version) echo {version};;\n*) echo 'Usage: yt-dlp [OPTIONS] URL'; echo '--ignore-config --extractor-args';;\nesac\n")).unwrap();
            std::fs::set_permissions(program, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        std::fs::create_dir_all(pending.join("_internal")).unwrap();
        std::fs::write(pending.join("_internal/Python"), b"runtime").unwrap();
        std::fs::write(pending.join("_internal/base_library.zip"), b"stdlib").unwrap();
        assert!(!downloads::reusable_layout(id, &target));
        let state = RequiredToolManager::new(storage.clone());
        let result = configure(id, &state, &ConfigureManager::default(), |p| {
            assert_ne!(p.phase, "preparing", "must reuse complete pending bundle");
        }).await.unwrap();
        assert!(result.error.is_none(), "{:?}", result.error);
        assert_eq!(result.active.unwrap().programs[0].version, "2026.09.25");
        assert_eq!(std::fs::read(target.join("_internal/Python")).unwrap(), b"runtime");
        assert!(downloads::reusable_layout(id, &target));
        assert!(!pending.exists());
        let result = configure(id, &state, &ConfigureManager::default(), |p| assert_ne!(p.phase, "preparing")).await.unwrap();
        assert!(result.error.is_none());
        root.close().unwrap();
    }

    #[tokio::test]
    async fn a_missing_managed_directory_or_program_is_not_an_invalid_manual_path() {
        let root = tempfile::tempdir().unwrap();
        eprintln!("missing-tool test directory: {}", root.path().display());
        let storage = Storage::new(
            &root.path().join("app.db"),
            &root.path().join("legacy.json"),
        );
        for id in [
            RequiredToolId::Ffmpeg,
            RequiredToolId::Ytdlp,
            RequiredToolId::Deno,
        ] {
            if supported(id) {
                assert_eq!(
                    detect_managed(id, &storage).await.unwrap_err().code,
                    "notFound"
                );
            }
        }
        if supported(RequiredToolId::Ffmpeg) {
            let directory = tool_directory(RequiredToolId::Ffmpeg, &storage).unwrap();
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(
                directory.join(executable_name("ffmpeg")),
                b"incomplete pair",
            )
                .unwrap();
            let failure = detect_managed(RequiredToolId::Ffmpeg, &storage)
                .await
                .unwrap_err();
            assert_eq!(failure.code, "notFound");
            assert_eq!(failure.program, "ffprobe");
        }
    }

    fn proxy_response(status: u16) -> (proxy::ProxySettings, std::thread::JoinHandle<String>) {
        use std::io::Read;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let settings = proxy::ProxySettings {
            protocol: "http".into(),
            address: "127.0.0.1".into(),
            port: listener.local_addr().unwrap().port(),
        };
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        && std::time::Instant::now() < deadline =>
                        {
                            std::thread::sleep(Duration::from_millis(2));
                        }
                    Err(e) => panic!("download proxy was not contacted: {e}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") && request.len() < 8192 {
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 {status} Test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    )
                        .as_bytes(),
                )
                .unwrap();
            String::from_utf8(request).unwrap()
        });
        (settings, server)
    }

    #[tokio::test]
    #[ignore = "child process fixture for isolated proxy environment"]
    async fn environment_download_proxy_fixture() {
        let root = tempfile::tempdir().unwrap();
        eprintln!("environment proxy test directory: {}", root.path().display());
        let storage = Storage::new(&root.path().join("app.db"), &root.path().join("legacy.json"));
        if std::env::var("EVD_TEST_UNAVAILABLE_PROXY").is_ok() {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let settings = proxy::ProxySettings {
                protocol: "http".into(),
                address: "127.0.0.1".into(),
                port: listener.local_addr().unwrap().port(),
            };
            drop(listener);
            storage.database().unwrap().save_proxy_settings(Some(&settings)).unwrap();
        }
        let http = client(&storage, RequiredToolId::Ytdlp).await.unwrap();
        // The test proxy deliberately rejects CONNECT, before any external TLS traffic.
        assert!(http.get("https://download-source.invalid/release").send().await.is_err());
        root.close().unwrap();
    }

    #[test]
    fn missing_or_unavailable_app_proxy_preserves_environment_proxy() {
        for unavailable in [false, true] {
            let (settings, server) = proxy_response(502);
            let mut child = std::process::Command::new(std::env::current_exe().unwrap());
            child.args([
                "--exact",
                "required_tools::managed::tests::environment_download_proxy_fixture",
                "--ignored", "--nocapture",
            ]);
            for key in ["HTTP_PROXY", "http_proxy", "HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy", "NO_PROXY", "no_proxy"] {
                child.env_remove(key);
            }
            child.env("HTTPS_PROXY", settings.url());
            child.env_remove("EVD_TEST_UNAVAILABLE_PROXY");
            if unavailable { child.env("EVD_TEST_UNAVAILABLE_PROXY", "1"); }
            let output = child.output().unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            assert!(server.join().unwrap().starts_with("CONNECT download-source.invalid:443 HTTP/1.1\r\n"));
        }
    }

    #[tokio::test]
    async fn automatic_downloads_use_only_a_working_configured_proxy() {
        for (status, accepted) in [(204, true), (302, true), (407, false), (503, false)] {
            let (settings, server) = proxy_response(status);
            let selected = download_proxy(
                Some(settings.clone()),
                "http://download-source.invalid/release",
            )
                .await;
            assert_eq!(selected, accepted.then_some(settings));
            assert!(server
                .join()
                .unwrap()
                .starts_with("HEAD http://download-source.invalid/release HTTP/1.1\r\n"));
        }
    }

    #[tokio::test]
    async fn an_absent_or_unreachable_download_proxy_leaves_selection_to_the_environment() {
        assert!(
            download_proxy(None, "http://download-source.invalid/release")
                .await
                .is_none()
        );
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let settings = proxy::ProxySettings {
            protocol: "http".into(),
            address: "127.0.0.1".into(),
            port: listener.local_addr().unwrap().port(),
        };
        drop(listener);
        assert!(
            download_proxy(Some(settings), "http://download-source.invalid/release")
                .await
                .is_none()
        );
    }

    #[test]
    fn acknowledged_cancel_blocks_commit_and_commit_rejects_late_cancel() {
        let manager = ConfigureManager::default();
        let (sender, receiver) = watch::channel(false);
        *manager.cancel[RequiredToolId::Ytdlp.index()]
            .lock()
            .unwrap() = Some(sender);
        assert!(manager.cancel(RequiredToolId::Ytdlp).unwrap());
        assert_eq!(
            manager
                .begin_commit(RequiredToolId::Ytdlp, &receiver)
                .unwrap_err()
                .code,
            "configureCancelled"
        );
        let (sender, receiver) = watch::channel(false);
        *manager.cancel[RequiredToolId::Ytdlp.index()]
            .lock()
            .unwrap() = Some(sender);
        manager
            .begin_commit(RequiredToolId::Ytdlp, &receiver)
            .unwrap();
        assert!(!manager.cancel(RequiredToolId::Ytdlp).unwrap());
        assert!(!*receiver.borrow());
    }

    #[test]
    fn cancellation_is_scoped_to_one_tool() {
        let manager = ConfigureManager::default();
        let (ytdlp, ytdlp_receiver) = watch::channel(false);
        let (ffmpeg, ffmpeg_receiver) = watch::channel(false);
        *manager.cancel[RequiredToolId::Ytdlp.index()]
            .lock()
            .unwrap() = Some(ytdlp);
        *manager.cancel[RequiredToolId::Ffmpeg.index()]
            .lock()
            .unwrap() = Some(ffmpeg);
        assert!(manager.cancel(RequiredToolId::Ffmpeg).unwrap());
        assert!(!*ytdlp_receiver.borrow());
        assert!(*ffmpeg_receiver.borrow());
        manager
            .begin_commit(RequiredToolId::Ytdlp, &ytdlp_receiver)
            .unwrap();
        assert_eq!(
            manager
                .begin_commit(RequiredToolId::Ffmpeg, &ffmpeg_receiver)
                .unwrap_err()
                .code,
            "configureCancelled"
        );
    }

    #[test]
    fn only_supported_platforms_select_standalone_assets() {
        for (os, arch, expected) in [
            ("windows", "x86_64", "yt-dlp.exe"),
            ("windows", "aarch64", "yt-dlp_arm64.exe"),
            ("macos", "x86_64", "yt-dlp_macos.zip"),
            ("macos", "aarch64", "yt-dlp_macos.zip"),
        ] {
            assert_eq!(asset(os, arch).unwrap(), expected);
        }
        assert_eq!(
            asset("linux", "x86_64").unwrap_err().code,
            "automaticUnsupported"
        );
        assert!(asset("windows", "unknown").is_err());
    }

    #[test]
    fn checksum_requires_exact_unambiguous_asset_and_valid_digest() {
        let valid = format!(
            "{}  yt-dlp.exe\n{}  yt-dlp_macos\n",
            "ab".repeat(32),
            "cd".repeat(32)
        );
        assert_eq!(checksum(&valid, "yt-dlp.exe").unwrap(), vec![0xab; 32]);
        for bad in [
            "abcd yt-dlp.exe".into(),
            valid.repeat(2),
            valid.replace("yt-dlp.exe", "other.exe"),
        ] {
            assert_eq!(
                checksum(&bad, "yt-dlp.exe").unwrap_err().code,
                "downloadInvalid"
            );
        }
    }

    #[test]
    fn unsuccessful_installation_restores_previous_executable_and_cleans_staging() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("yt-dlp.exe");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("ffmpeg"), b"previous").unwrap();
        std::fs::write(target.join("ffprobe"), b"previous probe").unwrap();
        let staging;
        {
            let mut installation = Installation::new(target.clone()).unwrap();
            staging = installation
                .directory
                .as_ref()
                .unwrap()
                .path()
                .to_path_buf();
            std::fs::write(installation.staged().join("ffmpeg"), b"replacement").unwrap();
            std::fs::write(installation.staged().join("ffprobe"), b"replacement probe").unwrap();
            installation.publish().unwrap();
            assert_eq!(
                std::fs::read(target.join("ffmpeg")).unwrap(),
                b"replacement"
            );
        }
        assert_eq!(std::fs::read(target.join("ffmpeg")).unwrap(), b"previous");
        assert_eq!(
            std::fs::read(target.join("ffprobe")).unwrap(),
            b"previous probe"
        );
        assert!(!staging.exists());
    }

    #[tokio::test]
    #[ignore = "downloads and executes official yt-dlp; run explicitly on a supported desktop OS"]
    async fn live_configures_official_standalone_ytdlp() {
        live_configuration(RequiredToolId::Ytdlp).await;
    }

    #[tokio::test]
    #[ignore = "downloads and executes the official Deno release"]
    async fn live_configures_deno() {
        live_configuration(RequiredToolId::Deno).await;
    }

    #[tokio::test]
    #[ignore = "downloads and executes FFmpeg and FFprobe from the provider listed by ffmpeg.org"]
    async fn live_configures_ffmpeg() {
        live_configuration(RequiredToolId::Ffmpeg).await;
    }

    async fn live_configuration(id: RequiredToolId) {
        let root = tempfile::tempdir().unwrap();
        eprintln!("{id:?} temporary test directory: {}", root.path().display());
        let storage = Storage::new(
            &root.path().join("app.db"),
            &root.path().join("legacy.json"),
        );
        // Use an existing test environment proxy only in this isolated test database.
        if let Ok(value) = std::env::var("HTTPS_PROXY") {
            let url = url::Url::parse(&value).unwrap();
            storage
                .database()
                .unwrap()
                .save_proxy_settings(Some(&proxy::ProxySettings {
                    protocol: url.scheme().into(),
                    address: url.host_str().unwrap().into(),
                    port: url.port_or_known_default().unwrap(),
                }))
                .unwrap();
        }
        let state = RequiredToolManager::new(storage.clone());
        let last = StdMutex::new("");
        let result = configure(id, &state, &ConfigureManager::default(), |progress| {
            let mut phase = last.lock().unwrap();
            if *phase != progress.phase {
                eprintln!("{id:?}: {}", progress.phase);
                *phase = progress.phase;
            }
        })
            .await
            .unwrap();
        assert!(result.error.is_none(), "{:?}", result.error);
        let active = result.active.unwrap();
        assert_eq!(active.source, RequiredToolSource::Automatic);
        assert!(active.programs[0].path.is_file());
        assert_eq!(
            storage.database().unwrap().tools().unwrap().tools[&id],
            active
        );
        assert!(detect_managed(id, &storage).await.is_ok());
        if id != RequiredToolId::Ytdlp {
            assert_eq!(active.programs.len(), id.names().len());
            assert!(active.programs.iter().all(|p| p.path.is_file()));
            return;
        }
        let mut without_python = tokio::process::Command::new(&active.programs[0].path);
        without_python
            .env("PATH", "")
            .env_remove("PYTHONHOME")
            .env_remove("PYTHONPATH")
            .args(["--ignore-config", "--version"]);
        let startup = std::time::Instant::now();
        assert_eq!(
            collect_output(without_python, Duration::from_secs(10))
                .await
                .unwrap()
                .trim(),
            active.programs[0].version
        );
        eprintln!("installed yt-dlp --version (without system Python): {:?}", startup.elapsed());
        root.close().unwrap();
    }
}
