use crate::database::{Storage, StorageError};
use crate::datetime;
use serde::{Deserialize, Serialize};
pub(crate) mod managed;
pub(crate) mod usage;

#[cfg(test)]
mod usage_contract_tests {
    #[test]
    #[cfg(windows)]
    fn differently_cased_tool_paths_share_the_same_usage_guard() {
        let root = tempfile::tempdir().unwrap();
        eprintln!(
            "temporary tool path guard directory: {}",
            root.path().display()
        );
        let file = root.path().join("yt-dlp.exe");
        std::fs::write(&file, b"tool").unwrap();
        let registry = super::usage::ToolUsageRegistry::default();
        let lease = registry.acquire(&[file]).unwrap();
        let alternate = std::path::PathBuf::from(root.path().to_string_lossy().to_uppercase());
        assert!(registry.publication(&alternate).is_err());
        drop(lease);
        assert!(registry.publication(&alternate).is_ok());
    }
    #[test]
    fn review_configuration_lock_covers_the_usage_capture_boundary() {
        let root = tempfile::tempdir().unwrap();
        eprintln!(
            "temporary atomic settings capture directory: {}",
            root.path().display()
        );
        let storage =
            crate::database::Storage::new(&root.path().join("app.db"), &root.path().join("legacy"));
        let tools = super::RequiredToolManager::new(storage);
        let (_settings, _lease) = tools
            .settings_and_usage_at(|| {
                assert!(
                    tools.settings.try_lock().is_err(),
                    "tool publication must not update settings between clone and usage acquisition"
                )
            })
            .unwrap();
    }
    #[test]
    fn managed_publication_cannot_replace_a_queued_tasks_tool() {
        let registry = super::usage::ToolUsageRegistry::default();
        let root = std::path::PathBuf::from("managed");
        let path = root.join("yt-dlp.exe");
        let lease = registry.acquire(std::slice::from_ref(&path)).unwrap();
        assert!(registry.publication(&root).is_err());
        assert!(registry
            .publication(&std::path::PathBuf::from("other"))
            .is_ok());
        drop(lease);
        let publish = registry.publication(&root).unwrap();
        assert!(registry.acquire(std::slice::from_ref(&path)).is_err());
        drop(publish);
        assert!(registry.acquire(&[path]).is_ok());
    }
}
pub(crate) mod process_tree;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};
use tauri_plugin_dialog::DialogExt;
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    sync::Mutex,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RequiredToolId {
    Ytdlp,
    Ffmpeg,
    #[serde(alias = "runtime")]
    Deno,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RequiredToolSource {
    Path,
    Manual,
    Automatic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequiredToolRequest {
    pub tool_id: RequiredToolId,
    pub source: RequiredToolSource,
    pub manual_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Program {
    pub name: String,
    pub path: PathBuf,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequiredToolConfig {
    pub source: RequiredToolSource,
    pub manual_path: String,
    pub programs: Vec<Program>,
    #[serde(deserialize_with = "datetime::deserialize_legacy")]
    pub checked_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequiredToolError {
    pub code: String,
    pub program: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequiredToolCheckState {
    pub source: RequiredToolSource,
    pub manual_path: String,
    pub error: Option<RequiredToolError>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequiredToolSettings {
    #[serde(alias = "dependencies")]
    pub tools: BTreeMap<RequiredToolId, RequiredToolConfig>,
}

fn error(code: &str, program: &str, detail: impl ToString) -> RequiredToolError {
    RequiredToolError {
        code: code.into(),
        program: program.into(),
        detail: detail.to_string(),
    }
}

impl RequiredToolId {
    fn names(self) -> &'static [&'static str] {
        match self {
            Self::Ytdlp => &["yt-dlp"],
            Self::Ffmpeg => &["ffmpeg", "ffprobe"],
            Self::Deno => &["deno"],
        }
    }
    fn index(self) -> usize {
        match self {
            Self::Ytdlp => 0,
            Self::Ffmpeg => 1,
            Self::Deno => 2,
        }
    }
}

fn parse_version(program: &str, output: &str) -> Result<String, RequiredToolError> {
    let line = output.lines().next().unwrap_or("").trim();
    let version = match program {
        "yt-dlp" => {
            let parts: Vec<_> = line.split('.').collect();
            if parts.len() >= 3
                && parts[0].len() == 4
                && parts
                    .iter()
                    .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
            {
                Some(line)
            } else {
                None
            }
        }
        "ffmpeg" | "ffprobe" => line
            .strip_prefix(&format!("{program} version "))
            .and_then(|s| s.split_whitespace().next())
            .filter(|s| s.chars().any(|c| c.is_ascii_digit())),
        "deno" => line
            .strip_prefix("deno ")
            .and_then(|s| s.split_whitespace().next()),
        _ => None,
    }
    .ok_or_else(|| error("invalidVersion", program, line))?;
    if program == "deno" {
        let numbers: Vec<u32> = version
            .split('.')
            .map(str::parse)
            .collect::<Result<_, _>>()
            .map_err(|_| error("invalidVersion", program, line))?;
        if numbers.len() != 3 {
            return Err(error("invalidVersion", program, line));
        }
        if (numbers[0], numbers[1], numbers[2]) < (2, 3, 0) {
            return Err(error("denoTooOld", program, version));
        }
    }
    Ok(version.into())
}

fn verify_identity(program: &str, output: &str) -> Result<(), RequiredToolError> {
    let has_line = |prefix: &str| {
        output
            .lines()
            .any(|line| line.trim_start().starts_with(prefix))
    };
    let matches =
        match program {
            "yt-dlp" => {
                output.lines().next().is_some_and(|line| {
                    line.starts_with("Usage: ") && line.contains("[OPTIONS] URL")
                }) && output.contains("--ignore-config")
                    && output.contains("--extractor-args")
            }
            "ffmpeg" | "ffprobe" => {
                has_line(&format!("{program} version "))
                    && ["libavutil ", "libavcodec ", "libavformat "]
                        .iter()
                        .all(|prefix| has_line(prefix))
            }
            "deno" => has_line("deno ") && has_line("v8 ") && has_line("typescript "),
            _ => false,
        };
    if matches {
        Ok(())
    } else {
        Err(error(
            "invalidProgram",
            program,
            output.chars().take(500).collect::<String>(),
        ))
    }
}

fn resolve_programs(
    request: &RequiredToolRequest,
) -> Result<Vec<(String, PathBuf)>, RequiredToolError> {
    let manual = Path::new(request.manual_path.trim());
    if request.source == RequiredToolSource::Manual
        && (!manual.is_absolute()
            || (request.tool_id == RequiredToolId::Ffmpeg && !manual.is_dir()))
    {
        return Err(error("invalidPath", "", request.manual_path.trim()));
    }
    request
        .tool_id
        .names()
        .iter()
        .map(|name| {
            let path = match request.source {
                RequiredToolSource::Path => {
                    which::which(name).map_err(|e| error("notFound", name, e))?
                }
                RequiredToolSource::Manual if request.tool_id == RequiredToolId::Ffmpeg => manual
                    .join(if cfg!(windows) {
                        format!("{name}.exe")
                    } else {
                        (*name).into()
                    }),
                RequiredToolSource::Manual => manual.to_path_buf(),
                RequiredToolSource::Automatic => {
                    return Err(error("automaticUnsupported", name, ""))
                }
            };
            if !path.is_file() {
                return Err(error("notFound", name, path.display()));
            }
            // On Windows, execute binaries directly; batch files would require a shell.
            if cfg!(windows)
                && !path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
            {
                return Err(error("invalidPath", name, path.display()));
            }
            let path = std::path::absolute(path).map_err(|e| error("invalidPath", name, e))?;
            Ok(((*name).into(), path))
        })
        .collect()
}

pub(crate) fn load_settings(path: &Path) -> Result<RequiredToolSettings, RequiredToolError> {
    match std::fs::read(path) {
        Ok(bytes) => {
            let settings: RequiredToolSettings =
                serde_json::from_slice(&bytes).map_err(|e| error("loadFailed", "", e))?;
            validate_settings(&settings)?;
            Ok(settings)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            match path.file_name().and_then(|name| name.to_str()) {
                Some("required-tools.json") => {
                    load_settings(&path.with_file_name("program-dependencies.json"))
                }
                Some("program-dependencies.json") => {
                    load_settings(&path.with_file_name("tools.json"))
                }
                _ => Ok(RequiredToolSettings::default()),
            }
        }
        Err(e) => Err(error("loadFailed", "", e)),
    }
}

pub(crate) fn validate_settings(settings: &RequiredToolSettings) -> Result<(), RequiredToolError> {
    for (id, config) in &settings.tools {
        let names = id.names();
        let valid = config.programs.len() == names.len()
            && config.programs.iter().zip(names).all(|(program, name)| {
                let header = match *name {
                    "yt-dlp" => program.version.clone(),
                    "deno" => format!("deno {}", program.version),
                    _ => format!("{name} version {}", program.version),
                };
                program.name == *name
                    && program.path.is_absolute()
                    && parse_version(name, &header).is_ok()
            })
            && (config.source != RequiredToolSource::Manual
                || Path::new(&config.manual_path).is_absolute())
            && datetime::is_valid(&config.checked_at);
        if !valid {
            return Err(error(
                "loadFailed",
                "",
                format!("invalid configuration for {id:?}"),
            ));
        }
    }
    Ok(())
}

async fn read_bounded(stream: impl AsyncRead + Unpin) -> Result<Vec<u8>, RequiredToolError> {
    const MAX_OUTPUT: u64 = 64 * 1024;
    let mut bytes = Vec::new();
    stream
        .take(MAX_OUTPUT + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|e| error("readFailed", "", e))?;
    if bytes.len() as u64 > MAX_OUTPUT {
        return Err(error("outputTooLarge", "", "64 KiB"));
    }
    Ok(bytes)
}

#[cfg(test)]
async fn collect_output(
    command: tokio::process::Command,
    limit: Duration,
) -> Result<String, RequiredToolError> {
    collect_output_with_cancel(command, limit, None).await
}

async fn collect_output_with_cancel(
    mut command: tokio::process::Command,
    limit: Duration,
    mut cancellation: Option<tokio::sync::watch::Receiver<bool>>,
) -> Result<String, RequiredToolError> {
    if cancellation
        .as_ref()
        .is_some_and(|receiver| *receiver.borrow())
    {
        return Err(error("configureCancelled", "", ""));
    }
    let diagnostic_command = format!("{:?}", command.as_std());
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let (mut child, tree) = process_tree::spawn(&mut command)
        .await
        .map_err(|e| error("spawnFailed", "", e))?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let result = tokio::select! {
        biased;
        _ = async {
            match cancellation.as_mut() {
                Some(receiver) => { let _ = receiver.changed().await; }
                None => std::future::pending::<()>().await,
            }
        } => None,
        result = tokio::time::timeout(limit, async {
        tokio::try_join!(
            async { child.wait().await.map_err(|e| error("readFailed", "", e)) },
            read_bounded(stdout),
            read_bounded(stderr)
        )
        }) => Some(result),
    };
    match result {
        Some(Ok(Ok((status, stdout, stderr)))) => {
            if !status.success() {
                return Err(error(
                    "exitFailed",
                    "",
                    format!(
                        "{status}: {}",
                        String::from_utf8_lossy(&stderr)
                            .chars()
                            .take(4096)
                            .collect::<String>()
                    ),
                ));
            }
            let output = if stdout.is_empty() { stderr } else { stdout };
            Ok(String::from_utf8_lossy(&output).into())
        }
        failure => {
            // Kill the whole process tree before waiting for the direct child.
            drop(tree);
            let _ = child.start_kill();
            let _ = child.wait().await;
            Err(match failure {
                Some(Ok(Err(e))) => e,
                Some(Err(_)) => error(
                    "timeout",
                    "",
                    format!("Command: {diagnostic_command}\nTimeout: {} ms", limit.as_millis()),
                ),
                None => error("configureCancelled", "", ""),
                _ => unreachable!(),
            })
        }
    }
}
fn apply_candidate(
    settings: &mut RequiredToolSettings,
    id: RequiredToolId,
    candidate: Result<RequiredToolConfig, RequiredToolError>,
    save: impl FnOnce(&RequiredToolConfig) -> Result<(), RequiredToolError>,
) -> Result<(), RequiredToolError> {
    let candidate = candidate?;
    save(&candidate)?;
    settings.tools.insert(id, candidate);
    Ok(())
}

fn storage_error(e: StorageError) -> RequiredToolError {
    error(&e.code, "", e.detail)
}

async fn detect(request: &RequiredToolRequest) -> Result<RequiredToolConfig, RequiredToolError> {
    detect_with_cancel(request, None).await
}

async fn detect_with_cancel(
    request: &RequiredToolRequest,
    cancellation: Option<tokio::sync::watch::Receiver<bool>>,
) -> Result<RequiredToolConfig, RequiredToolError> {
    let mut programs = Vec::new();
    for (name, path) in resolve_programs(request)? {
        // The macOS standalone build may need substantially longer on startup.
        let limit = Duration::from_secs(if cfg!(target_os = "macos") && name == "yt-dlp" {
            60
        } else {
            10
        });
        let mut command = tokio::process::Command::new(&path);
        if name == "yt-dlp" {
            command.arg("--ignore-config");
        }
        command.arg(if matches!(name.as_str(), "ffmpeg" | "ffprobe") {
            "-version"
        } else {
            "--version"
        });
        let output =
            collect_output_with_cancel(command, limit, cancellation.clone())
                .await
                .map_err(|mut e| {
                    e.program = name.clone();
                    e
                })?;
        let version = parse_version(&name, &output)?;
        let identity_output = if name == "yt-dlp" {
            let mut help = tokio::process::Command::new(&path);
            help.args(["--ignore-config", "--help"]);
            collect_output_with_cancel(help, limit, cancellation.clone())
                .await
                .map_err(|mut e| {
                    e.program = name.clone();
                    e
                })?
        } else {
            output
        };
        verify_identity(&name, &identity_output)?;
        programs.push(Program {
            version,
            name,
            path,
        });
    }
    Ok(RequiredToolConfig {
        source: request.source,
        manual_path: if request.source == RequiredToolSource::Manual {
            request.manual_path.trim().into()
        } else {
            String::new()
        },
        programs,
        checked_at: datetime::now(),
    })
}

pub struct RequiredToolManager {
    pub(crate) usage: usage::ToolUsageRegistry,
    settings: Arc<StdMutex<RequiredToolSettings>>,
    checks: [Mutex<()>; 3],
    storage: Storage,
    load_error: Option<RequiredToolError>,
}

pub(crate) fn validate_program_files(
    id: RequiredToolId,
    config: &RequiredToolConfig,
) -> Result<(), RequiredToolError> {
    for name in id.names() {
        let program = config
            .programs
            .iter()
            .find(|program| program.name == *name)
            .ok_or_else(|| error("notFound", name, ""))?;
        match std::fs::metadata(&program.path) {
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => return Err(error("notFound", name, program.path.display())),
            Err(failure) if failure.kind() == std::io::ErrorKind::NotFound => {
                return Err(error("notFound", name, program.path.display()));
            }
            Err(failure) => return Err(error("readFailed", name, failure)),
        }
    }
    Ok(())
}

impl RequiredToolManager {
    pub(crate) fn acquire_usage(
        &self,
        settings: &RequiredToolSettings,
    ) -> Result<usage::ToolUsageLease, RequiredToolError> {
        self.usage.acquire(
            &settings
                .tools
                .values()
                .flat_map(|c| c.programs.iter().map(|p| p.path.clone()))
                .collect::<Vec<_>>(),
        )
    }

    pub(crate) fn settings_and_usage(
        &self,
    ) -> Result<(RequiredToolSettings, usage::ToolUsageLease), RequiredToolError> {
        self.settings_and_usage_at(|| {})
    }
    fn settings_and_usage_at(
        &self,
        boundary: impl FnOnce(),
    ) -> Result<(RequiredToolSettings, usage::ToolUsageLease), RequiredToolError> {
        if let Some(error) = &self.load_error {
            return Err(error.clone());
        }
        let guard = self
            .settings
            .lock()
            .map_err(|e| error("loadFailed", "", e))?;
        let settings = guard.clone();
        boundary();
        let lease = self.acquire_usage(&settings)?;
        drop(guard);
        Ok((settings, lease))
    }
    pub(crate) fn settings_snapshot(&self) -> Result<RequiredToolSettings, RequiredToolError> {
        if let Some(error) = &self.load_error {
            return Err(error.clone());
        }
        self.settings
            .lock()
            .map(|settings| settings.clone())
            .map_err(|e| error("loadFailed", "", e))
    }
    pub fn new(storage: Storage) -> Self {
        let (settings, load_error) = match storage
            .database()
            .and_then(|db| db.tools())
            .map_err(storage_error)
        {
            Ok(settings) => (settings, None),
            Err(e) => (RequiredToolSettings::default(), Some(e)),
        };
        Self {
            usage: usage::ToolUsageRegistry::default(),
            settings: Arc::new(StdMutex::new(settings)),
            checks: std::array::from_fn(|_| Mutex::new(())),
            storage,
            load_error,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequiredToolSettingsSnapshot {
    settings: RequiredToolSettings,
    last_checks: BTreeMap<RequiredToolId, RequiredToolCheckState>,
    error: Option<RequiredToolError>,
    automatic_supported: BTreeMap<RequiredToolId, bool>,
    automatic_ffmpeg_requires_rosetta: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    active: Option<RequiredToolConfig>,
    error: Option<RequiredToolError>,
}

#[tauri::command]
pub async fn get_required_tools(
    state: tauri::State<'_, RequiredToolManager>,
) -> Result<RequiredToolSettingsSnapshot, RequiredToolError> {
    let settings = state.settings.clone();
    let load_error = state.load_error.clone();
    let storage = state.storage.clone();
    tauri::async_runtime::spawn_blocking(move || {
        settings_view_snapshot(&settings, &storage, load_error)
    })
        .await
        .map_err(|e| error("loadFailed", "", e))?
}

fn settings_view_snapshot(
    settings: &StdMutex<RequiredToolSettings>,
    storage: &Storage,
    load_error: Option<RequiredToolError>,
) -> Result<RequiredToolSettingsSnapshot, RequiredToolError> {
    // Hold the same lock used for publishing successful checks while reading both.
    let settings = settings.lock().map_err(|e| error("loadFailed", "", e))?;
    let mut last_checks = if load_error.is_none() {
        storage
            .database()
            .and_then(|db| db.tool_checks())
            .map_err(storage_error)?
    } else {
        BTreeMap::new()
    };
    if load_error.is_none() {
        for (id, config) in &settings.tools {
            // Preserve an unsuccessful newer selection. Only refresh the applied
            // selection's file availability; never launch programs while loading.
            if last_checks.get(id).is_some_and(|check| {
                check.source != config.source
                    || check.manual_path != config.manual_path
                    || check.error.is_some()
            }) {
                continue;
            }
            if let Err(failure) = validate_program_files(*id, config) {
                let check = RequiredToolCheckState {
                    source: config.source,
                    manual_path: config.manual_path.clone(),
                    error: Some(failure),
                };
                storage
                    .database()
                    .and_then(|db| db.save_tool_check(*id, &check, None))
                    .map_err(storage_error)?;
                last_checks.insert(*id, check);
            }
        }
    }
    Ok(RequiredToolSettingsSnapshot {
        settings: settings.clone(),
        last_checks,
        error: load_error,
        automatic_supported: [
            RequiredToolId::Ytdlp,
            RequiredToolId::Ffmpeg,
            RequiredToolId::Deno,
        ]
            .into_iter()
            .map(|id| (id, managed::supported(id)))
            .collect(),
        automatic_ffmpeg_requires_rosetta: cfg!(all(target_os = "macos", target_arch = "aarch64")),
    })
}

#[tauri::command]
pub async fn check_required_tool(
    request: RequiredToolRequest,
    state: tauri::State<'_, RequiredToolManager>,
) -> Result<CheckResult, RequiredToolError> {
    check_tool(&request, &state).await
}

async fn check_tool(
    request: &RequiredToolRequest,
    state: &RequiredToolManager,
) -> Result<CheckResult, RequiredToolError> {
    let _checking = state.checks[request.tool_id.index()]
        .try_lock()
        .map_err(|_| error("busy", "", ""))?;
    // A corrupt/unreadable existing file must not be silently overwritten.
    if let Some(e) = &state.load_error {
        return Err(e.clone());
    }
    let candidate = if request.source == RequiredToolSource::Automatic {
        managed::detect_managed(request.tool_id, &state.storage).await
    } else {
        detect(request).await
    };
    apply_result(state, request, candidate).await
}

async fn apply_result(
    state: &RequiredToolManager,
    request: &RequiredToolRequest,
    candidate: Result<RequiredToolConfig, RequiredToolError>,
) -> Result<CheckResult, RequiredToolError> {
    let settings = state.settings.clone();
    let storage = state.storage.clone();
    let tool_id = request.tool_id;
    let last_check = match &candidate {
        Ok(config) => RequiredToolCheckState {
            source: config.source,
            manual_path: config.manual_path.clone(),
            error: None,
        },
        Err(failure) => RequiredToolCheckState {
            source: request.source,
            manual_path: if request.source == RequiredToolSource::Manual {
                request.manual_path.trim().into()
            } else {
                String::new()
            },
            error: Some(failure.clone()),
        },
    };
    tauri::async_runtime::spawn_blocking(move || {
        // Publish the new in-memory configuration only after SQLite commits.
        let mut settings = settings.lock().map_err(|e| error("saveFailed", "", e))?;
        let database = storage.database().map_err(storage_error)?;
        let failure = match candidate {
            Ok(config) => apply_candidate(&mut settings, tool_id, Ok(config), |config| {
                database
                    .save_tool_check(tool_id, &last_check, Some(config))
                    .map_err(storage_error)
            })
                .err(),
            Err(failure) => Some(match database.save_tool_check(tool_id, &last_check, None) {
                Ok(()) => failure,
                Err(error) => storage_error(error),
            }),
        };
        Ok(CheckResult {
            active: settings.tools.get(&tool_id).cloned(),
            error: failure,
        })
    })
    .await
    .map_err(|e| error("saveFailed", "", e))?
}

#[tauri::command]
pub async fn select_required_tool_path(
    app: tauri::AppHandle,
    window: tauri::Window,
    tool_id: RequiredToolId,
) -> Result<Option<String>, RequiredToolError> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let dialog = app.dialog().file().set_parent(&window);
    let callback = move |path: Option<tauri_plugin_dialog::FilePath>| {
        let _ = sender.send(path);
    };
    if tool_id == RequiredToolId::Ffmpeg {
        dialog.pick_folder(callback);
    } else {
        dialog.pick_file(callback);
    }
    receiver
        .await
        .map_err(|e| error("dialogFailed", "", e))?
        .map(|p| {
            p.into_path()
                .map(|p| p.to_string_lossy().into_owned())
                .map_err(|e| error("dialogFailed", "", e))
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config(version: &str) -> RequiredToolConfig {
        let path = std::env::temp_dir().join(if cfg!(windows) {
            "yt-dlp.exe"
        } else {
            "yt-dlp"
        });
        RequiredToolConfig {
            source: RequiredToolSource::Manual,
            manual_path: path.to_string_lossy().into(),
            programs: vec![Program {
                name: "yt-dlp".into(),
                path,
                version: version.into(),
            }],
            checked_at: "1970-01-01 08:02:03".into(),
        }
    }

    #[test]
    fn recognizes_real_version_headers() {
        assert_eq!(
            parse_version("yt-dlp", "2026.09.25\n").unwrap(),
            "2026.09.25"
        );
        assert_eq!(
            parse_version(
                "ffmpeg",
                "ffmpeg version 8.0-full_build Copyright\nconfiguration: test"
            )
            .unwrap(),
            "8.0-full_build"
        );
        assert_eq!(
            parse_version("ffprobe", "ffprobe version N-120000-g123 Copyright").unwrap(),
            "N-120000-g123"
        );
        assert_eq!(
            parse_version(
                "deno",
                "deno 2.3.0 (stable, release, x86_64-pc-windows-msvc)\nv8 1\ntypescript 1"
            )
            .unwrap(),
            "2.3.0"
        );
    }

    #[test]
    fn rejects_wrong_program_and_old_deno() {
        assert_eq!(
            parse_version("ffprobe", "ffmpeg version 8.0")
                .unwrap_err()
                .code,
            "invalidVersion"
        );
        assert_eq!(
            parse_version("yt-dlp", "node v22.0.0").unwrap_err().code,
            "invalidVersion"
        );
        assert_eq!(
            parse_version("deno", "deno 2.2.9").unwrap_err().code,
            "denoTooOld"
        );
    }

    #[test]
    fn ffmpeg_directory_requires_both_programs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(if cfg!(windows) {
                "ffmpeg.exe"
            } else {
                "ffmpeg"
            }),
            "",
        )
        .unwrap();
        let request = RequiredToolRequest {
            tool_id: RequiredToolId::Ffmpeg,
            source: RequiredToolSource::Manual,
            manual_path: dir.path().to_string_lossy().into(),
        };
        let failure = resolve_programs(&request).unwrap_err();
        assert_eq!(failure.code, "notFound");
        assert_eq!(failure.program, "ffprobe");
    }

    #[test]
    fn failed_candidate_keeps_previous_configuration() {
        let mut settings = RequiredToolSettings::default();
        settings.tools.insert(RequiredToolId::Ytdlp, config("old"));
        let before = settings.clone();
        assert!(apply_candidate(
            &mut settings,
            RequiredToolId::Ytdlp,
            Err(error("timeout", "yt-dlp", "")),
            |_| panic!("failed candidate must not write")
        )
        .is_err());
        assert_eq!(settings, before);
    }

    #[test]
    fn opening_tool_settings_marks_deleted_ffmpeg_files_as_missing() {
        let root = tempfile::tempdir().unwrap();
        eprintln!(
            "deleted-tool settings test directory: {}",
            root.path().display()
        );
        let storage = Storage::new(
            &root.path().join("app.db"),
            &root.path().join("legacy.json"),
        );
        for source in [
            RequiredToolSource::Path,
            RequiredToolSource::Automatic,
            RequiredToolSource::Manual,
        ] {
            for missing in ["ffmpeg", "ffprobe"] {
                let config = RequiredToolConfig {
                    source,
                    manual_path: if source == RequiredToolSource::Manual {
                        root.path().to_string_lossy().into()
                    } else {
                        String::new()
                    },
                    checked_at: datetime::now(),
                    programs: ["ffmpeg", "ffprobe"]
                        .into_iter()
                        .map(|name| {
                            let path = root.path().join(if cfg!(windows) {
                                format!("{name}.exe")
                            } else {
                                name.into()
                            });
                            std::fs::write(&path, b"file metadata fixture; never executed")
                                .unwrap();
                            Program {
                                name: name.into(),
                                path,
                                version: "9.0.2".into(),
                            }
                        })
                        .collect(),
                };
                storage
                    .database()
                    .unwrap()
                    .save_tool_check(
                        RequiredToolId::Ffmpeg,
                        &RequiredToolCheckState {
                            source,
                            manual_path: config.manual_path.clone(),
                            error: None,
                        },
                        Some(&config),
                    )
                    .unwrap();
                let manager = RequiredToolManager::new(storage.clone());
                assert!(settings_view_snapshot(&manager.settings, &storage, None)
                    .unwrap()
                    .last_checks[&RequiredToolId::Ffmpeg]
                    .error
                    .is_none());
                let path = &config
                    .programs
                    .iter()
                    .find(|p| p.name == missing)
                    .unwrap()
                    .path;
                std::fs::remove_file(path).unwrap();
                let snapshot = settings_view_snapshot(&manager.settings, &storage, None).unwrap();
                let check = &snapshot.last_checks[&RequiredToolId::Ffmpeg];
                assert_eq!(check.source, source);
                let failure = check
                    .error
                    .as_ref()
                    .expect("deleted executable must not remain installed");
                assert_eq!(failure.code, "notFound");
                assert_eq!(failure.program, missing);
                assert_eq!(snapshot.settings.tools[&RequiredToolId::Ffmpeg], config);
                let reopened = RequiredToolManager::new(storage.clone());
                assert_eq!(
                    settings_view_snapshot(&reopened.settings, &storage, None)
                        .unwrap()
                        .last_checks[&RequiredToolId::Ffmpeg],
                    *check
                );
            }
        }
    }

    #[tokio::test]
    async fn failed_tool_checks_restore_their_selection_and_status_after_reopening() {
        let root = tempfile::tempdir().unwrap();
        eprintln!(
            "tool-check restart test directory: {}",
            root.path().display()
        );
        let path = root.path().join("app.db");
        let legacy = root.path().join("legacy.json");
        let storage = Storage::new(&path, &legacy);
        let ids = [
            RequiredToolId::Ytdlp,
            RequiredToolId::Ffmpeg,
            RequiredToolId::Deno,
        ];
        let mut previous = BTreeMap::new();
        for id in ids.into_iter().filter(|id| managed::supported(*id)) {
            let config = RequiredToolConfig {
                source: RequiredToolSource::Path,
                manual_path: String::new(),
                checked_at: datetime::now(),
                programs: id
                    .names()
                    .iter()
                    .map(|name| Program {
                        name: (*name).into(),
                        path: root.path().join(format!("old-{name}.exe")),
                        version: if id == RequiredToolId::Ytdlp {
                            "2026.09.25"
                        } else {
                            "9.0.2"
                        }
                            .into(),
                    })
                    .collect(),
            };
            storage.database().unwrap().save_tool(id, &config).unwrap();
            previous.insert(id, config);
        }
        let manager = RequiredToolManager::new(storage);
        for id in previous.keys() {
            let request = RequiredToolRequest {
                tool_id: *id,
                source: RequiredToolSource::Automatic,
                manual_path: String::new(),
            };
            let result = check_tool(&request, &manager).await.unwrap();
            assert_eq!(result.error.unwrap().code, "notFound");
            assert_eq!(result.active.as_ref(), previous.get(id));
        }
        drop(manager);
        let restarted = RequiredToolManager::new(Storage::new(&path, &legacy));
        let snapshot = settings_view_snapshot(
            &restarted.settings,
            &restarted.storage,
            restarted.load_error.clone(),
        )
            .unwrap();
        for id in previous.keys() {
            let check = &snapshot.last_checks[id];
            assert_eq!(check.source, RequiredToolSource::Automatic);
            assert_eq!(check.manual_path, "");
            assert_eq!(check.error.as_ref().unwrap().code, "notFound");
        }
        if let Some(config) = previous.get(&RequiredToolId::Ytdlp) {
            // This leg tests persistence only; the restored program must also
            // exist for the settings page's file-availability check.
            std::fs::write(
                &config.programs[0].path,
                b"available fixture; never executed",
            )
                .unwrap();
            let configured = RequiredToolConfig {
                source: RequiredToolSource::Automatic,
                ..config.clone()
            };
            let request = RequiredToolRequest {
                tool_id: RequiredToolId::Ytdlp,
                source: RequiredToolSource::Automatic,
                manual_path: String::new(),
            };
            let result = apply_result(&restarted, &request, Ok(configured))
                .await
                .unwrap();
            assert!(result.error.is_none());
            drop(restarted);
            let configured = RequiredToolManager::new(Storage::new(&path, &legacy));
            let snapshot = settings_view_snapshot(
                &configured.settings,
                &configured.storage,
                configured.load_error.clone(),
            )
                .unwrap();
            assert!(snapshot.last_checks[&RequiredToolId::Ytdlp].error.is_none());
            assert_eq!(
                snapshot.settings.tools[&RequiredToolId::Ytdlp].source,
                RequiredToolSource::Automatic
            );
        }
    }

    #[test]
    fn failed_save_keeps_previous_configuration() {
        let mut settings = RequiredToolSettings::default();
        settings.tools.insert(RequiredToolId::Ytdlp, config("old"));
        let before = settings.clone();
        assert_eq!(
            apply_candidate(
                &mut settings,
                RequiredToolId::Ytdlp,
                Ok(config("new")),
                |_| Err(error("saveFailed", "", "read-only database"))
            )
            .unwrap_err()
            .code,
            "saveFailed"
        );
        assert_eq!(settings, before);
    }

    #[test]
    fn saved_configuration_is_restored_without_running_programs() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::database::Database::open(
            &dir.path().join("app.db"),
            &dir.path().join("required-tools.json"),
        )
        .unwrap();
        let mut settings = RequiredToolSettings::default();
        apply_candidate(
            &mut settings,
            RequiredToolId::Ytdlp,
            Ok(config("2026.09.25")),
            |candidate| {
                db.save_tool(RequiredToolId::Ytdlp, candidate)
                    .map_err(storage_error)
            },
        )
        .unwrap();
        assert_eq!(db.tools().unwrap(), settings);
        let mut deno = config("2.5.0");
        deno.programs[0].name = "deno".into();
        apply_candidate(&mut settings, RequiredToolId::Deno, Ok(deno), |candidate| {
            db.save_tool(RequiredToolId::Deno, candidate)
                .map_err(storage_error)
        })
        .unwrap();
        assert_eq!(db.tools().unwrap().tools.len(), 2);
    }

    #[test]
    fn corrupt_configuration_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("tools.json");
        std::fs::write(&file, "not JSON").unwrap();
        assert_eq!(load_settings(&file).unwrap_err().code, "loadFailed");
    }

    #[test]
    fn reads_legacy_file_without_overwriting_it() {
        for (filename, field, id) in [
            ("program-dependencies.json", "dependencies", "deno"),
            ("tools.json", "tools", "runtime"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("required-tools.json");
            let legacy = dir.path().join(filename);
            let original = serde_json::json!({ (field): { (id): {
            "source": "manual", "manualPath": std::env::temp_dir().join("deno.exe"),
            "programs": [{"name": "deno", "path": std::env::temp_dir().join("deno.exe"), "version": "2.5.0"}], "checkedAt": 123
        }}}).to_string();
            std::fs::write(&legacy, &original).unwrap();
            let settings = load_settings(&path).unwrap();
            assert!(
                settings.tools.contains_key(&RequiredToolId::Deno),
                "legacy settings were lost during rename"
            );
            assert!(!path.exists(), "reading must not write configuration");
            assert_eq!(std::fs::read_to_string(&legacy).unwrap(), original);
        }
    }

    #[test]
    fn invalid_saved_programs_are_not_restored() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("dependencies.json");
        for change in 0..4 {
            let mut candidate = config("2026.08.19");
            match change {
                0 => candidate.programs.clear(),
                1 => candidate.programs[0].name = "node".into(),
                2 => candidate.programs[0].path = "relative.exe".into(),
                _ => candidate.checked_at = "2026-02-30 00:00:00".into(),
            }
            let mut settings = RequiredToolSettings::default();
            settings.tools.insert(RequiredToolId::Ytdlp, candidate);
            let original = serde_json::to_vec(&settings).unwrap();
            std::fs::write(&file, &original).unwrap();
            assert!(
                matches!(load_settings(&file), Err(e) if e.code == "loadFailed"),
                "accepted invalid variant {change}"
            );
            assert_eq!(std::fs::read(&file).unwrap(), original);
        }
    }

    #[test]
    fn program_identity_requires_characteristic_output() {
        assert!(verify_identity(
            "yt-dlp",
            "Usage: yt-dlp [OPTIONS] URL [URL...]\n--ignore-config\n--extractor-args"
        )
        .is_ok());
        assert!(verify_identity(
            "ffmpeg",
            "ffmpeg version 9.0\nlibavutil 61\nlibavcodec 63\nlibavformat 63"
        )
        .is_ok());
        assert!(verify_identity("deno", "deno 2.5.0\nv8 12.0\ntypescript 5.8").is_ok());
        for (name, fake) in [
            ("yt-dlp", "2026.08.19"),
            ("ffmpeg", "node v24.0.0"),
            ("ffprobe", "ffprobe version 9.0"),
            ("deno", "deno 2.5.0"),
        ] {
            assert!(
                matches!(verify_identity(name, fake), Err(e) if e.code == "invalidProgram"),
                "accepted fake {name}"
            );
        }
    }

    #[test]
    fn ytdlp_identity_accepts_packaged_and_renamed_executables() {
        for name in ["yt-dlp.exe", "yt-dlp_x86.exe", "my-downloader.exe"] {
            let help =
                format!("Usage: {name} [OPTIONS] URL [URL...]\n--ignore-config\n--extractor-args");
            assert!(verify_identity("yt-dlp", &help).is_ok(), "rejected {name}");
        }
    }

    fn fixture(mode: &str) -> tokio::process::Command {
        let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
        command.args([
            "--ignored",
            "--exact",
            "required_tools::tests::process_fixture",
            "--nocapture",
        ]);
        command.env("EVD_TEST_PROCESS", mode);
        command
    }

    #[test]
    #[ignore = "child process fixture, launched by process tests"]
    fn process_fixture() {
        match std::env::var("EVD_TEST_PROCESS").unwrap().as_str() {
            "sleep" => {
                std::thread::sleep(std::time::Duration::from_secs(2));
                std::fs::write(std::env::var("EVD_TEST_MARKER").unwrap(), "alive").unwrap();
            }
            "flood" => {
                println!("{}", "x".repeat(70_000));
            }
            "exit" => {
                eprintln!("fixture failed");
                std::process::exit(7);
            }
            "descendant" => {
                std::thread::sleep(Duration::from_millis(50));
                let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--ignored",
                        "--exact",
                        "required_tools::tests::process_fixture",
                        "--nocapture",
                    ])
                    .env("EVD_TEST_PROCESS", "sleep")
                    .spawn()
                    .unwrap();
                let _ = child.wait();
            }
            _ => panic!("unknown fixture"),
        }
    }

    #[tokio::test]
    async fn cancellation_waits_for_validation_process_before_removing_its_executable() {
        let directory = tempfile::tempdir().unwrap();
        let staged = directory.path().join(if cfg!(windows) {
            "validation.exe"
        } else {
            "validation"
        });
        std::fs::copy(std::env::current_exe().unwrap(), &staged).unwrap();
        let mut command = tokio::process::Command::new(&staged);
        command
            .args([
                "--ignored",
                "--exact",
                "required_tools::tests::process_fixture",
                "--nocapture",
            ])
            .env("EVD_TEST_PROCESS", "sleep")
            .env("EVD_TEST_MARKER", directory.path().join("alive"));
        let (sender, receiver) = tokio::sync::watch::channel(false);
        let cancellation = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            sender.send(true).unwrap();
        });
        let failure = collect_output_with_cancel(command, Duration::from_secs(10), Some(receiver))
            .await
            .unwrap_err();
        cancellation.await.unwrap();
        assert_eq!(failure.code, "configureCancelled");
        std::fs::remove_file(&staged)
            .expect("validation process must release its executable before cleanup");
        directory.close().unwrap();
    }

    #[tokio::test]
    async fn timeout_terminates_the_process() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("must-not-exist");
        let mut command = fixture("sleep");
        command.env("EVD_TEST_MARKER", &marker);
        let result = collect_output(command, std::time::Duration::from_millis(150)).await;
        let failure = result.unwrap_err();
        assert_eq!(failure.code, "timeout");
        assert!(failure.detail.contains("150 ms"), "{}", failure.detail);
        assert!(failure.detail.contains("process_fixture"), "{}", failure.detail);
        tokio::time::sleep(std::time::Duration::from_millis(2200)).await;
        assert!(!marker.exists(), "timed-out process was left running");
    }

    #[tokio::test]
    async fn timeout_also_terminates_descendant_processes() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("descendant-must-not-exist");
        let mut command = fixture("descendant");
        command.env("EVD_TEST_MARKER", &marker);
        let failure = collect_output(command, Duration::from_millis(300))
            .await
            .unwrap_err();
        assert_eq!(failure.code, "timeout");
        tokio::time::sleep(Duration::from_millis(2200)).await;
        assert!(
            !marker.exists(),
            "descendant was left running after timeout"
        );
    }

    #[tokio::test]
    async fn rejects_unbounded_process_output() {
        assert_eq!(
            collect_output(fixture("flood"), std::time::Duration::from_secs(3))
                .await
                .unwrap_err()
                .code,
            "outputTooLarge"
        );
    }

    #[tokio::test]
    async fn nonzero_exit_is_a_failure_with_diagnostic() {
        let failure = collect_output(fixture("exit"), std::time::Duration::from_secs(3))
            .await
            .unwrap_err();
        assert_eq!(failure.code, "exitFailed");
        assert!(failure.detail.contains("fixture failed"));
    }
}
