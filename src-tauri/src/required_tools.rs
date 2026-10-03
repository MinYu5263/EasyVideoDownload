use crate::database::{Storage, StorageError};
use serde::{Deserialize, Serialize};
mod process_tree;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex as StdMutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
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
    pub checked_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequiredToolError {
    pub code: String,
    pub program: String,
    pub detail: String,
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
            && config.checked_at <= 8_640_000_000_000;
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

async fn collect_output(
    mut command: tokio::process::Command,
    limit: Duration,
) -> Result<String, RequiredToolError> {
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
    let result = tokio::time::timeout(limit, async {
        tokio::try_join!(
            async { child.wait().await.map_err(|e| error("readFailed", "", e)) },
            read_bounded(stdout),
            read_bounded(stderr)
        )
    })
    .await;
    match result {
        Ok(Ok((status, stdout, stderr))) => {
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
                Ok(Err(e)) => e,
                Err(_) => error("timeout", "", "10 seconds"),
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
    let mut programs = Vec::new();
    for (name, path) in resolve_programs(request)? {
        let mut command = tokio::process::Command::new(&path);
        if name == "yt-dlp" {
            command.arg("--ignore-config");
        }
        command.arg(if matches!(name.as_str(), "ffmpeg" | "ffprobe") {
            "-version"
        } else {
            "--version"
        });
        let output = collect_output(command, Duration::from_secs(10))
            .await
            .map_err(|mut e| {
                e.program = name.clone();
                e
            })?;
        let version = parse_version(&name, &output)?;
        let identity_output = if name == "yt-dlp" {
            let mut help = tokio::process::Command::new(&path);
            help.args(["--ignore-config", "--help"]);
            collect_output(help, Duration::from_secs(10))
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
        checked_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    })
}

pub struct RequiredToolManager {
    settings: Arc<StdMutex<RequiredToolSettings>>,
    checks: [Mutex<()>; 3],
    storage: Storage,
    load_error: Option<RequiredToolError>,
}

impl RequiredToolManager {
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
    error: Option<RequiredToolError>,
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
    tauri::async_runtime::spawn_blocking(move || {
        Ok(RequiredToolSettingsSnapshot {
            settings: settings
                .lock()
                .map_err(|e| error("loadFailed", "", e))?
                .clone(),
            error: load_error,
        })
    })
    .await
    .map_err(|e| error("loadFailed", "", e))?
}

#[tauri::command]
pub async fn check_required_tool(
    request: RequiredToolRequest,
    state: tauri::State<'_, RequiredToolManager>,
) -> Result<CheckResult, RequiredToolError> {
    let _checking = state.checks[request.tool_id.index()]
        .try_lock()
        .map_err(|_| error("busy", "", ""))?;
    // A corrupt/unreadable existing file must not be silently overwritten.
    if let Some(e) = &state.load_error {
        return Err(e.clone());
    }
    let candidate = detect(&request).await;
    let settings = state.settings.clone();
    let storage = state.storage.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Publish the new in-memory configuration only after SQLite commits.
        let mut settings = settings.lock().map_err(|e| error("saveFailed", "", e))?;
        let failure = apply_candidate(&mut settings, request.tool_id, candidate, |config| {
            storage
                .database()
                .and_then(|db| db.save_tool(request.tool_id, config))
                .map_err(storage_error)
        })
        .err();
        Ok(CheckResult {
            active: settings.tools.get(&request.tool_id).cloned(),
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
            checked_at: 123,
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
                _ => candidate.checked_at = u64::MAX,
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
    async fn timeout_terminates_the_process() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("must-not-exist");
        let mut command = fixture("sleep");
        command.env("EVD_TEST_MARKER", &marker);
        let result = collect_output(command, std::time::Duration::from_millis(150)).await;
        assert_eq!(result.unwrap_err().code, "timeout");
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
