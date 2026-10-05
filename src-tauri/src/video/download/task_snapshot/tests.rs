use super::*;

#[test]
fn paused_record_resume_uses_current_attempt_specs_instead_of_previous_success() {
    use crate::database::{
        download_records::{tasks::RecordAcceptance, DownloadRecordOutcome, DownloadSnapshot},
        persistence_tests::page,
        Database,
    };
    let root = tempfile::tempdir().unwrap();
    eprintln!("owned paused snapshot fixture: {}", root.path().display());
    let db = Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap();
    let mut state = page();
    state.download_directory = root.path().join("original").to_string_lossy().into();
    let RecordAcceptance::Accepted(first) = db
        .accept_download_record(
            "first",
            &DownloadSnapshot {
                page: state.clone(),
            },
            false,
            false,
        )
        .unwrap()
    else {
        panic!()
    };
    db.finish_download_record(
        "first",
        &DownloadRecordOutcome::Completed {
            path: root
                .path()
                .join("original/video.webm")
                .to_string_lossy()
                .into(),
            size: 8,
            extension: Some("webm".into()),
        },
    )
        .unwrap();
    state.formats[1].height = Some(720);
    state.selected_height = Some(720);
    state.download_directory = root.path().join("current").to_string_lossy().into();
    db.accept_download_record(
        "paused",
        &DownloadSnapshot {
            page: state.clone(),
        },
        false,
        true,
    )
        .unwrap();
    db.finish_download_record("paused", &DownloadRecordOutcome::Paused)
        .unwrap();
    let record = db.get_download_record(first.id).unwrap();
    assert!(record.successful_output.is_some());
    let execution = page_from_record(&record).unwrap();
    assert_eq!(execution.download_directory, state.download_directory);
    assert_eq!(execution.formats[0].height, Some(720));
    assert_eq!(execution.selected_format_id.as_deref(), Some("b"));
}

#[test]
fn recorded_execution_uses_snapshot_selector_when_history_identity_is_legacy() {
    use crate::database::{
        download_records::{tasks::RecordAcceptance, DownloadRecordOutcome, DownloadSnapshot},
        persistence_tests::page,
        Database,
    };
    for completed in [false, true] {
        let root = tempfile::tempdir().unwrap();
        eprintln!(
            "owned legacy selector reconstruction directory: {}",
            root.path().display()
        );
        let db = Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap();
        let mut state = page();
        state.download_directory = root.path().to_string_lossy().into();
        let RecordAcceptance::Accepted(row) = db
            .accept_download_record("legacy", &DownloadSnapshot { page: state }, false, false)
            .unwrap()
        else {
            panic!()
        };
        if completed {
            let output = root.path().join("video.webm");
            std::fs::write(&output, b"media").unwrap();
            db.finish_download_record(
                "legacy",
                &DownloadRecordOutcome::Completed {
                    path: output.to_string_lossy().into(),
                    size: 5,
                    extension: Some("webm".into()),
                },
            )
                .unwrap();
        }
        let mut actual = row.format_snapshot.clone().unwrap();
        actual.format_id = "canonical-selector".into();
        let mut stored = db.get_download_record(row.id).unwrap();
        stored.format_snapshot = Some(actual.clone());
        if let Some(success) = stored.successful_output.as_mut() {
            success.format_snapshot = Some(actual);
        }
        assert_eq!(stored.format_id, row.format_id);
        let execution = page_from_record(&stored).unwrap();
        assert_eq!(
            execution.selected_format_id.as_deref(),
            Some("canonical-selector")
        );
        assert_eq!(execution.formats[0].format_id, "canonical-selector");
    }
}
#[tokio::test]
async fn queued_task_keeps_cookie_proxy_selected_format_and_directory() {
    use crate::required_tools::{Program, RequiredToolConfig, RequiredToolSource};
    let root = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary immutable task directory: {}",
        root.path().display()
    );
    let storage = Storage::new(&root.path().join("app.db"), &root.path().join("legacy"));
    let db = storage.database().unwrap();
    let executable = std::env::current_exe().unwrap();
    for (id, names) in [
        (RequiredToolId::Ytdlp, vec!["yt-dlp"]),
        (RequiredToolId::Ffmpeg, vec!["ffmpeg", "ffprobe"]),
        (RequiredToolId::Deno, vec!["deno"]),
    ] {
        db.save_tool(
            id,
            &RequiredToolConfig {
                source: RequiredToolSource::Manual,
                manual_path: executable.to_string_lossy().into(),
                programs: names
                    .into_iter()
                    .map(|name| Program {
                        name: name.into(),
                        path: executable.clone(),
                        version: if name == "yt-dlp" {
                            "2026.10.04".into()
                        } else {
                            "8.0.0".into()
                        },
                    })
                    .collect(),
                checked_at: crate::datetime::now(),
            },
        )
            .unwrap();
    }
    let tools = RequiredToolManager::new(storage.clone());
    let cookies = CookieStore::new(root.path());
    cookies
        .save(CookiePlatform::Youtube, "original cookie")
        .unwrap();
    let proxy: crate::proxy::ProxySettings = serde_json::from_value(
        serde_json::json!({"protocol":"http","address":"127.0.0.1","port":7890}),
    )
        .unwrap();
    db.save_proxy_settings(Some(&proxy)).unwrap();
    db.save_platform_settings(
        CookiePlatform::Youtube,
        &crate::database::platform_settings::PlatformSettings {
            proxy_enabled: true,
        },
    )
        .unwrap();
    let mut page = crate::database::persistence_tests::page();
    page.download_directory = root
        .path()
        .join("original destination")
        .to_string_lossy()
        .into();
    page.parser_fingerprint = Some(
        crate::database::page_states::parser_fingerprint(&tools.settings_snapshot().unwrap())
            .unwrap(),
    );
    let mut snapshot = capture_task_snapshot(page.clone(), &tools, &cookies, &storage)
        .await
        .unwrap();
    let copy = snapshot._cookie.as_ref().unwrap().path().to_path_buf();
    eprintln!("temporary pinned Cookie file: {}", copy.display());
    let mut future_tool = snapshot.settings.tools[&RequiredToolId::Ytdlp].clone();
    future_tool.programs[0].path = root.path().join("future-yt-dlp.exe");
    future_tool.manual_path = future_tool.programs[0].path.to_string_lossy().into();
    db.save_tool(RequiredToolId::Ytdlp, &future_tool).unwrap();
    let future_tools = RequiredToolManager::new(storage.clone());
    assert_eq!(
        future_tools.settings_snapshot().unwrap().tools[&RequiredToolId::Ytdlp].programs[0].path,
        future_tool.programs[0].path
    );
    assert_eq!(
        snapshot.ytdlp_command().as_std().get_program(),
        executable.as_os_str()
    );
    cookies
        .save(CookiePlatform::Youtube, "future cookie")
        .unwrap();
    db.save_proxy_settings(None).unwrap();
    page.download_directory = root
        .path()
        .join("future destination")
        .to_string_lossy()
        .into();
    page.selected_format_id = Some("a".into());
    let temporary_directory = root.path().join("owned temporary directory");
    snapshot
        .set_temporary_directory(&temporary_directory)
        .unwrap();
    let args = snapshot
        .ytdlp_command()
        .as_std()
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(args.iter().any(|arg| arg.contains("[format_id=\"b\"]")));
    assert!(args.iter().any(|arg| arg.contains("original destination")));
    assert!(args.iter().any(|arg| arg.contains("127.0.0.1:7890")));
    let network: serde_json::Value = serde_json::from_str(&snapshot.network_diagnostic).unwrap();
    assert_eq!(network["proxyEnabled"], true);
    assert_eq!(network["proxyAddress"], "127.0.0.1");
    assert_eq!(network["proxyPort"], 7890);
    assert_eq!(network["proxyArgument"], "--proxy");
    let temporary_arg = format!("temp:{}", temporary_directory.display());
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--paths", &temporary_arg]));
    assert!(
        args.iter().position(|arg| arg == &temporary_arg).unwrap()
            < args.iter().position(|arg| arg == "--").unwrap()
    );
    assert!(!args.iter().any(|arg| arg == "--merge-output-format"));
    assert_eq!(std::fs::read_to_string(&copy).unwrap(), "original cookie");
    assert_eq!(
        cookies.load(CookiePlatform::Youtube).unwrap(),
        "future cookie"
    );
    assert!(tools
        .usage
        .publication(executable.parent().unwrap())
        .is_err());
    drop(snapshot);
    assert!(!copy.exists());
    assert!(tools
        .usage
        .publication(executable.parent().unwrap())
        .is_ok());
}

#[test]
fn recorded_redownload_uses_last_successful_specs_without_changing_the_page() {
    use crate::database::{
        download_records::{tasks::RecordAcceptance, DownloadRecordOutcome, DownloadSnapshot},
        persistence_tests::page,
        Database,
    };
    let root = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary record command reconstruction directory: {}",
        root.path().display()
    );
    let db = Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap();
    let mut page = page();
    page.formats[1].video_codec = Some("hev1.1".into());
    page.formats[1].width = Some(1920);
    page.download_directory = root.path().to_string_lossy().into();
    db.save_download_page_state(&page).unwrap();
    let RecordAcceptance::Accepted(row) = db
        .accept_download_record(
            "first",
            &DownloadSnapshot { page: page.clone() },
            false,
            false,
        )
        .unwrap()
    else {
        panic!()
    };
    db.finish_download_record(
        "first",
        &DownloadRecordOutcome::Completed {
            path: root.path().join("old.webm").to_string_lossy().into(),
            size: 8,
            extension: Some("webm".into()),
        },
    )
        .unwrap();
    let original_directory = page.download_directory.clone();
    page.selected_format_id = Some("a".into());
    page.download_directory = root.path().join("changed").to_string_lossy().into();
    db.accept_download_record("failed", &DownloadSnapshot { page }, false, true)
        .unwrap();
    db.finish_download_record("failed", &DownloadRecordOutcome::Cancelled)
        .unwrap();
    let retry = page_from_record(&db.get_download_record(row.id).unwrap()).unwrap();
    assert_eq!(retry.selected_format_id.as_deref(), Some("b"));
    assert_eq!(retry.download_directory, original_directory);
    assert_eq!(retry.formats[0].extension.as_deref(), Some("webm"));
    assert_eq!(retry.formats[0].video_codec.as_deref(), Some("hev1.1"));
    assert_eq!(retry.formats[0].width, Some(1920));
    assert_eq!(
        db.download_page_states()
            .unwrap()
            .into_iter()
            .find(|p| p.platform == "youtube")
            .unwrap()
            .selected_format_id
            .as_deref(),
        Some("b")
    );
}

#[tokio::test]
async fn old_douyin_page_requires_native_reparse_before_task_creation() {
    let root = tempfile::tempdir().unwrap();
    eprintln!("owned stale Douyin page fixture: {}", root.path().display());
    let storage = Storage::new(&root.path().join("app.db"), &root.path().join("legacy"));
    let tools = RequiredToolManager::new(storage.clone());
    let cookies = CookieStore::new(root.path());
    let mut page = crate::database::persistence_tests::page();
    page.platform = "douyin".into();
    page.parser_fingerprint = Some("old-yt-dlp-fingerprint".into());
    let failure = capture_task_snapshot(page, &tools, &cookies, &storage)
        .await
        .err()
        .unwrap();
    assert_eq!(failure.code, "invalidDownloadOptions");
}
