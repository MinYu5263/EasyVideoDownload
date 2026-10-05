use super::*;
use std::{fs, path::PathBuf};

fn root() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    eprintln!(
        "owned native plugin log test directory: {}",
        root.path().display()
    );
    root
}
fn store(root: &Path) -> AppLogStore {
    let (logger, handle) = builder(root, false).unwrap().build().unwrap();
    AppLogStore::new(logger.into(), handle)
}
fn entry(message: &str) -> LogEntry {
    super::entry(
        "info",
        "douyin",
        Some("123"),
        None,
        "parseCompleted",
        None,
        message.into(),
    )
}

#[test]
fn diagnostics_are_appended_to_a_plain_log_in_the_current_data_directory() {
    let root = root();
    let store = store(root.path());
    store.append(entry("formats=38")).unwrap();
    let disk = fs::read_to_string(root.path().join("logs/application_rCURRENT.log")).unwrap();
    assert!(disk.contains("parseCompleted"));
    assert!(disk.contains("formats=38"));
    assert!(disk.contains("[INFO]"));
    assert!(disk.contains("[EasyVideoDownload::events]"));
    assert!(!root.path().join("logs/application.json").exists());
}

#[test]
fn native_module_logs_display_the_product_name_instead_of_the_library_name() {
    let root = root();
    let (logger, _handle) = builder(root.path(), false).unwrap().build().unwrap();
    logger.log(
        &Record::builder()
            .level(Level::Info)
            .target(module_path!())
            .args(format_args!("native-module-record"))
            .build(),
    );
    logger.flush();
    let disk = fs::read_to_string(root.path().join("logs/application_rCURRENT.log")).unwrap();
    assert!(disk.contains("[EasyVideoDownload::app_logs::tests] native-module-record"));
}

#[test]
fn native_crate_root_logs_display_the_product_name_instead_of_the_library_name() {
    let root = root();
    let (logger, _handle) = builder(root.path(), false).unwrap().build().unwrap();
    logger.log(
        &Record::builder()
            .level(Level::Info)
            .target(env!("CARGO_CRATE_NAME"))
            .args(format_args!("native-root-record"))
            .build(),
    );
    logger.flush();
    let disk = fs::read_to_string(root.path().join("logs/application_rCURRENT.log")).unwrap();
    assert!(disk.contains("[EasyVideoDownload] native-root-record"));
}

#[test]
fn logging_redacts_cookies_without_discarding_other_diagnostics() {
    let root = root();
    let store = store(root.path());
    store.append(entry("Cookie: session=private-cookie\nproxy connection refused\npassword=diagnostic-value\nhttps://media.test/video?token=diagnostic-token\nsignature=private-signature\nX-Api-Key: private-key\nAuthentication: private-auth")).unwrap();
    let disk = fs::read_to_string(root.path().join("logs/application_rCURRENT.log")).unwrap();
    assert!(!disk.contains("private-cookie"));
    assert!(disk.contains("proxy connection refused"));
    assert!(!disk.contains("diagnostic-value"));
    assert!(!disk.contains("diagnostic-token"));
    assert!(!disk.contains("https://media.test"));
    assert!(!disk.contains("private-"));
    assert!(disk.contains("parseCompleted"));
    assert_eq!(disk.lines().count(), 1);
}

#[test]
fn long_cookie_failures_keep_error_code_and_task_correlation() {
    let root = root();
    let store = store(root.path());
    let mut failure = super::entry(
        "error",
        "bilibili",
        Some("BV123"),
        Some("request-long"),
        "downloadFailed",
        Some("cookieReadFailed"),
        "x".repeat(6000),
    );
    failure.revision = Some(9);
    store.append(failure).unwrap();
    let disk = fs::read_to_string(root.path().join("logs/application_rCURRENT.log")).unwrap();
    let payload: serde_json::Value = serde_json::from_str(
        disk.lines()
            .last()
            .unwrap()
            .split_once("] ")
            .unwrap()
            .1
            .split_once("] ")
            .unwrap()
            .1,
    )
        .expect("structured log payload must remain complete JSON");
    assert_eq!(payload["requestId"], "request-long");
    assert_eq!(payload["videoId"], "BV123");
    assert_eq!(payload["platform"], "bilibili");
    assert_eq!(payload["code"], "cookieReadFailed");
    assert_eq!(payload["revision"], 9);
    assert!(payload["message"].as_str().unwrap().chars().count() <= 4096);
}

#[test]
fn imported_netscape_rows_and_json_cookie_fields_are_redacted_on_disk() {
    let root = root();
    let store = store(root.path());
    store
        .append(entry(
            ".douyin.com\tTRUE\t/\tTRUE\t0\tsession\tprivate-netscape-value",
        ))
        .unwrap();

    let (logger, _handle) = builder(root.path(), false).unwrap().build().unwrap();
    logger.log(
        &Record::builder()
            .level(Level::Error)
            .target("webview")
            .args(format_args!(
                r#"{{"headers":{{"Cookie":"private-header"}},"event":"startupFailed"}}"#
            ))
            .build(),
    );
    logger.flush();
    let disk = fs::read_to_string(root.path().join("logs/application_rCURRENT.log")).unwrap();
    assert!(!disk.contains("private-netscape-value"));
    assert!(!disk.contains("private-header"));
    assert!(disk.contains("startupFailed"));
}

#[test]
fn reopening_appends_without_overwriting_existing_logs_or_old_json() {
    let root = root();
    fs::create_dir_all(root.path().join("logs")).unwrap();
    let legacy = root.path().join("logs/application.json");
    fs::write(&legacy, b"previous version log").unwrap();
    {
        let store = store(root.path());
        store.append(entry("first-session")).unwrap();
    }
    let store = store(root.path());
    store.append(entry("second-session")).unwrap();
    let disk = fs::read_to_string(root.path().join("logs/application_rCURRENT.log")).unwrap();
    assert!(disk.contains("first-session"));
    assert!(disk.contains("second-session"));
    assert_eq!(fs::read(legacy).unwrap(), b"previous version log");
}

#[test]
fn concurrent_downloads_write_complete_records_without_loss() {
    let root = root();
    let store = Arc::new(store(root.path()));
    let threads = (0..4)
        .map(|worker| {
            let store = store.clone();
            std::thread::spawn(move || {
                for row in 0..10 {
                    store
                        .append(entry(&format!("worker={worker},row={row}")))
                        .unwrap();
                }
            })
        })
        .collect::<Vec<_>>();
    for thread in threads {
        thread.join().unwrap();
    }
    let disk = fs::read_to_string(root.path().join("logs/application_rCURRENT.log")).unwrap();
    assert_eq!(disk.lines().count(), 40);
    for worker in 0..4 {
        for row in 0..10 {
            assert_eq!(
                disk.matches(&format!("worker={worker},row={row}\""))
                    .count(),
                1
            );
        }
    }
}

#[test]
fn stale_or_duplicate_phases_cannot_follow_a_newer_terminal_stage() {
    let root = root();
    let store = store(root.path());
    let mut terminal = entry("cancelled");
    terminal.event = "downloadStage".into();
    terminal.request_id = Some("request-1".into());
    terminal.revision = Some(3);
    store.append(terminal.clone()).unwrap();
    store.append(terminal.clone()).unwrap();
    terminal.message = "cancelling".into();
    terminal.revision = Some(2);
    store.append(terminal).unwrap();
    let mut other = entry("queued");
    other.event = "downloadStage".into();
    other.request_id = Some("request-2".into());
    other.revision = Some(1);
    store.append(other).unwrap();
    let disk = fs::read_to_string(root.path().join("logs/application_rCURRENT.log")).unwrap();
    assert_eq!(disk.lines().count(), 2);
    assert!(disk.contains("cancelled"));
    assert!(disk.contains("queued"));
    assert!(!disk.contains("cancelling"));
}

#[test]
fn release_filters_debug_but_development_records_it() {
    for development in [false, true] {
        let root = root();
        let (logger, _handle) = builder(root.path(), development).unwrap().build().unwrap();
        for (level, message) in [(Level::Info, "info-record"), (Level::Debug, "debug-record")] {
            logger.log(
                &Record::builder()
                    .level(level)
                    .target("EasyVideoDownload::startup")
                    .args(format_args!("{message}"))
                    .build(),
            );
        }
        logger.log(
            &Record::builder()
                .level(Level::Error)
                .target("webview::main.ts")
                .args(format_args!("frontend-startup-error"))
                .build(),
        );
        logger.log(
            &Record::builder()
                .level(Level::Info)
                .target("reqwest")
                .args(format_args!("http-internals"))
                .build(),
        );
        logger.flush();
        let disk = fs::read_to_string(root.path().join("logs/application_rCURRENT.log")).unwrap();
        assert!(disk.contains("info-record"));
        assert!(disk.contains("frontend-startup-error"));
        assert_eq!(disk.contains("debug-record"), development);
        assert!(!disk.contains("http-internals"));
    }
}

fn log_files(root: &Path) -> Vec<PathBuf> {
    fs::read_dir(root.join("logs"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "log"))
        .collect()
}

#[test]
fn configured_size_rotates_and_retains_four_archives_without_touching_other_files() {
    let root = root();
    fs::create_dir_all(root.path().join("logs")).unwrap();
    let unrelated = root.path().join("logs/unrelated.log");
    fs::write(&unrelated, b"do not remove").unwrap();

    let (logger, _handle) = builder(root.path(), false).unwrap().build().unwrap();
    // Exercise the production 5 MiB limit and retention settings with actual writes.
    let payload = "x".repeat(4000);
    for index in 0..12_000 {
        logger.log(
            &Record::builder()
                .level(Level::Info)
                .target("EasyVideoDownload::events")
                .args(format_args!("record={index} {payload}"))
                .build(),
        );
    }
    logger.flush();
    let files = log_files(root.path())
        .into_iter()
        .filter(|path| path != &unrelated)
        .collect::<Vec<_>>();
    assert_eq!(files.len(), 5, "four archives plus the active log");
    for path in &files {
        assert!(fs::metadata(path).unwrap().len() <= 5 * 1024 * 1024 + 8192);
    }
    let active = fs::read_to_string(root.path().join("logs/application_rCURRENT.log")).unwrap();
    assert!(active.contains("record=11999"));
    assert_eq!(fs::read(unrelated).unwrap(), b"do not remove");
    assert!(!files
        .iter()
        .any(|path| fs::read_to_string(path).unwrap().contains("record=0 ")));
}

#[test]
fn native_log_initialization_reports_an_unwritable_directory() {
    let root = root();
    fs::write(root.path().join("logs"), b"blocking file").unwrap();

    assert!(builder(root.path(), false)
        .and_then(|logger| logger.build())
        .is_err());
    assert_eq!(
        fs::read(root.path().join("logs")).unwrap(),
        b"blocking file"
    );
}

#[test]
fn an_unopenable_current_log_is_reported_before_initialization_succeeds() {
    let root = root();
    fs::create_dir_all(root.path().join("logs/application_rCURRENT.log")).unwrap();
    assert!(builder(root.path(), false)
        .and_then(|logger| logger.build())
        .is_err());
}
