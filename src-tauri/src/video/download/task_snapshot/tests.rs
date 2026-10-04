use super::*;
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
    let snapshot = capture_task_snapshot(page.clone(), &tools, &cookies, &storage)
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
        snapshot.command.as_std().get_program(),
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
    let args = snapshot
        .command
        .as_std()
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(args.iter().any(|arg| arg.contains("[format_id=\"b\"]")));
    assert!(args.iter().any(|arg| arg.contains("original destination")));
    assert!(args.iter().any(|arg| arg.contains("127.0.0.1:7890")));
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
async fn douyin_snapshot_keeps_quality_and_fps_when_the_format_suffix_changes() {
    use crate::required_tools::{Program, RequiredToolConfig, RequiredToolSource};
    let root = tempfile::Builder::new()
        .prefix("evd-format-suffix-")
        .tempdir()
        .unwrap();
    eprintln!("owned format suffix fixture: {}", root.path().display());
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
    let mut page = crate::database::persistence_tests::page();
    page.platform = "douyin".into();
    page.input_link = "https://www.douyin.com/video/7688950275142389027".into();
    page.video_id = Some("7688950275142389027".into());
    page.formats = vec![crate::video::VideoFormat {
        format_id: "bytevc1_720p_764922-3".into(),
        height: Some(720),
        fps: Some(30.0),
        extension: Some("mp4".into()),
        size_bytes: Some(16384),
        size_approximate: false,
    }];
    page.selected_format_id = Some("bytevc1_720p_764922-3".into());
    page.selected_height = Some(720);
    page.selected_fps = Some(30.0);
    page.download_directory = root.path().to_string_lossy().into();
    page.parser_fingerprint = Some(
        crate::database::page_states::parser_fingerprint(&tools.settings_snapshot().unwrap())
            .unwrap(),
    );
    let snapshot = capture_task_snapshot(page.clone(), &tools, &cookies, &storage)
        .await
        .unwrap();
    let args = snapshot
        .command
        .as_std()
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let selector = &args.windows(2).find(|pair| pair[0] == "--format").unwrap()[1];
    eprintln!(
        "owned Douyin selector: {}",
        serde_json::to_string(selector).unwrap()
    );
    assert!(
        selector.contains("[height=720]"),
        "fallback must not change resolution: {selector}"
    );
    assert!(
        selector.contains("[fps=30]"),
        "fallback must not change frame rate: {selector}"
    );
    assert!(
        selector.contains("[ext=\"mp4\"]"),
        "fallback must preserve the container: {selector}"
    );
    assert!(
        selector.contains("[format_id^=\"bytevc1_720p_764922-\"]"),
        "fallback must stay in the same codec and bitrate family: {selector}"
    );
    page.selected_fps = None;
    page.formats[0].fps = None;
    let unknown = capture_task_snapshot(page, &tools, &cookies, &storage)
        .await
        .unwrap();
    let unknown_args = unknown
        .command
        .as_std()
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let unknown_selector = &unknown_args
        .windows(2)
        .find(|pair| pair[0] == "--format")
        .unwrap()[1];
    assert_eq!(
        unknown_selector,
        r#"bestvideo[format_id="bytevc1_720p_764922-3"]+bestaudio/best*[format_id="bytevc1_720p_764922-3"]"#,
        "unknown frame rates must not enable a broader mirror fallback"
    );
}
