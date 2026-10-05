use flexi_logger::{
    Cleanup, Criterion, Duplicate, FileSpec, Logger, LoggerHandle, Naming, WriteMode,
};
use log::{Level, Log, Metadata, Record};
use serde::Serialize;
use std::{
    collections::VecDeque,
    path::Path,
    sync::{Arc, Mutex},
};
use tauri::{Manager, Runtime};

const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;
const ARCHIVE_COUNT: usize = 4;
const TRACKED_REQUESTS: usize = 500;

pub(crate) fn builder(
    root: &Path,
    development: bool,
) -> Result<Logger, flexi_logger::FlexiLoggerError> {
    let directory = root.join("logs");
    std::fs::create_dir_all(&directory)?;
    // flexi_logger opens files lazily. Report an invalid/locked current file now,
    // before setup succeeds and logging failures become invisible in release builds.
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("application_rCURRENT.log"))?;
    let level = if development { "debug" } else { "info" };
    Ok(Logger::try_with_str(format!(
        "off,EasyVideoDownload={level},{}={level},webview={level}",
        env!("CARGO_CRATE_NAME")
    ))?
        .log_to_file(
            FileSpec::default()
                .directory(root.join("logs"))
                .basename("application")
                .suppress_timestamp(),
        )
        .append()
        // Numeric archives avoid collisions even when several rotations occur in one second.
        .rotate(
            Criterion::Size(MAX_FILE_BYTES),
            Naming::Numbers,
            Cleanup::KeepLogFiles(ARCHIVE_COUNT),
        )
        .cleanup_in_background_thread(false)
        .write_mode(WriteMode::Direct)
        .panic_if_error_channel_is_broken(false)
        .duplicate_to_stdout(if development {
            Duplicate::All
        } else {
            Duplicate::None
        })
        .format(|out, _now, record| {
            // Present the application name while retaining the Rust module for diagnostics.
            let target = if record.target() == env!("CARGO_CRATE_NAME") {
                "EasyVideoDownload".to_owned()
            } else {
                record
                    .target()
                    .strip_prefix(concat!(env!("CARGO_CRATE_NAME"), "::"))
                    .map(|module| format!("EasyVideoDownload::{module}"))
                    .unwrap_or_else(|| record.target().to_owned())
            };
            let safe = redact_cookies(&record.args().to_string())
                .replace('\r', "\\r")
                .replace('\n', "\\n");
            write!(
                out,
                "{} [{}] [{}] {}",
                crate::datetime::now(),
                record.level(),
                target,
                safe
            )
        }))
}

pub(crate) fn initialize<R: Runtime>(
    app: &tauri::AppHandle<R>,
    root: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let (logger, handle) = builder(root, cfg!(debug_assertions))?.build()?;
    let logger: Arc<dyn Log> = logger.into();
    let level = if cfg!(debug_assertions) {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };
    tauri_plugin_log::attach_logger(level, Box::new(SharedLogger(logger.clone())))?;
    app.plugin(tauri_plugin_log::Builder::new().skip_logger().build())?;
    app.manage(AppLogStore::new(logger, handle));
    log::info!(
        "applicationStarted version={} mode={}",
        app.package_info().version,
        if cfg!(debug_assertions) {
            "development"
        } else {
            "release"
        }
    );
    Ok(())
}

// Ordinary Rust logs, frontend logs and task events share the plugin's writer.
struct SharedLogger(Arc<dyn Log>);
impl Log for SharedLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.0.enabled(metadata)
    }
    fn log(&self, record: &Record<'_>) {
        self.0.log(record);
    }
    fn flush(&self) {
        self.0.flush();
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LogEntry {
    pub level: String,
    pub platform: Option<String>,
    pub video_id: Option<String>,
    pub request_id: Option<String>,
    pub event: String,
    pub code: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
}

pub(crate) struct AppLogStore {
    logger: Arc<dyn Log>,
    _handle: LoggerHandle,
    revisions: Mutex<VecDeque<(String, u64)>>,
}
impl AppLogStore {
    pub(crate) fn new(logger: Arc<dyn Log>, handle: LoggerHandle) -> Self {
        Self {
            logger,
            _handle: handle,
            revisions: Mutex::new(VecDeque::new()),
        }
    }
    pub(crate) fn append(&self, entry: LogEntry) -> Result<(), String> {
        // Cancellation can settle before an older phase is published. Serialize
        // the revision check and write so a stale phase cannot follow a terminal one.
        let mut revisions = self.revisions.lock().map_err(|e| e.to_string())?;
        if entry.event == "downloadStage" {
            if let (Some(request), Some(revision)) = (&entry.request_id, entry.revision) {
                if let Some(index) = revisions.iter().position(|(saved, _)| saved == request) {
                    if revisions[index].1 >= revision {
                        return Ok(());
                    }
                    revisions.remove(index);
                }
                revisions.push_back((request.clone(), revision));
                if revisions.len() > TRACKED_REQUESTS {
                    revisions.pop_front();
                }
            }
        }
        let level = entry.level.parse::<Level>().unwrap_or(Level::Info);
        let message = serde_json::to_string(&entry).map_err(|e| e.to_string())?;
        self.logger.log(
            &Record::builder()
                .level(level)
                .target("EasyVideoDownload::events")
                .args(format_args!("{message}"))
                .build(),
        );
        self.logger.flush();
        Ok(())
    }
}

pub(crate) fn record(app: &tauri::AppHandle, entry: LogEntry) {
    if let Some(store) = app.try_state::<AppLogStore>() {
        if let Err(error) = store.append(entry) {
            log::error!("Unable to record task diagnostic: {error}");
        }
    }
}

pub(crate) fn entry(
    level: &str,
    platform: &str,
    video_id: Option<&str>,
    request_id: Option<&str>,
    event: &str,
    code: Option<&str>,
    message: String,
) -> LogEntry {
    LogEntry {
        level: level.into(),
        platform: Some(platform.into()),
        video_id: video_id.map(str::to_owned),
        request_id: request_id.map(str::to_owned),
        event: event.into(),
        code: code.map(str::to_owned),
        message,
        revision: None,
    }
}

fn redact_cookie_lines(message: &str) -> String {
    message
        .lines()
        .map(|line| {
            let columns: Vec<_> = line.split('\t').collect();
            let netscape = columns.len() == 7
                && matches!(columns[1], "TRUE" | "FALSE")
                && matches!(columns[3], "TRUE" | "FALSE");
            if netscape || line.to_ascii_lowercase().contains("cookie") {
                "[Cookie redacted]"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn safe_text(message: &str) -> String {
    crate::video::download::failure::sanitize_diagnostic(&redact_cookie_lines(message))
}

fn redact_cookies(message: &str) -> String {
    fn redact_value(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(fields) => {
                for (key, field) in fields {
                    let name = key.to_ascii_lowercase();
                    if ["cookie", "authorization", "authentication", "password", "passwd", "token", "secret", "signature", "api_key", "api-key", "apikey"].iter().any(|part| name.contains(part)) {
                        *field = serde_json::Value::String("[redacted]".into());
                    } else if matches!(
                        key.as_str(),
                        "code" | "event" | "level" | "platform" | "requestId" | "videoId"
                    ) {
                        // These application-generated identifiers are diagnostic context,
                        // including codes such as cookieReadFailed, never Cookie contents.
                    } else {
                        redact_value(field);
                    }
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    redact_value(value);
                }
            }
            serde_json::Value::String(text) => {
                *text = safe_text(text);
            }
            _ => {}
        }
    }
    let safe = match serde_json::from_str::<serde_json::Value>(message) {
        Ok(mut value) => {
            redact_value(&mut value);
            value.to_string()
        }
        Err(_) => safe_text(message),
    };
    // Limit detail fields before serialization. Keep task IDs and valid JSON intact.
    safe
}

#[cfg(test)]
#[path = "app_logs/tests.rs"]
mod tests;
