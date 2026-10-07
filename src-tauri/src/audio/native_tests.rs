use super::*;

#[cfg(windows)]
#[test]
#[ignore = "requires installed FFmpeg/FFprobe and a native Tauri runtime"]
fn native_audio_commands_parse_extract_reject_invalid_track_and_clear() {
    use std::os::windows::process::CommandExt;
    let directory = tempfile::Builder::new()
        .prefix("evd-audio-native-")
        .tempdir()
        .unwrap();
    // Tauri/Wry owns COM resources until process exit. Isolate this fixture from other tests.
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "audio::native_tests::native_audio_fixture",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("EVD_AUDIO_TEST_DIRECTORY", directory.path())
        .creation_flags(0x08000000)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "Native audio command fixture: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(windows)]
#[test]
#[ignore = "native child fixture; invoked by native_audio_commands test"]
fn native_audio_fixture() {
    tauri::async_runtime::block_on(async {
        use crate::required_tools::{
            Program, RequiredToolConfig, RequiredToolId, RequiredToolManager, RequiredToolSource,
        };
        let directory =
            std::path::PathBuf::from(std::env::var_os("EVD_AUDIO_TEST_DIRECTORY").unwrap());
        let ffmpeg = which::which("ffmpeg").unwrap();
        let ffprobe = which::which("ffprobe").unwrap();
        let source = directory.join("访谈录制.mp4");
        let mut command = tokio::process::Command::new(&ffmpeg);
        command
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=size=32x32:duration=1",
                "-f",
                "lavfi",
                "-i",
                "sine=duration=1",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=880:duration=1",
                "-map",
                "0:v",
                "-map",
                "1:a",
                "-map",
                "2:a",
                "-metadata:s:a:0",
                "language=chi",
                "-metadata:s:a:1",
                "language=eng",
                "-disposition:a:0",
                "default",
                "-disposition:a:1",
                "0",
                "-c:v",
                "mpeg4",
                "-c:a",
                "aac",
            ])
            .arg(&source);
        let (mut child, tree) = crate::required_tools::process_tree::spawn(&mut command)
            .await
            .unwrap();
        assert!(child.wait().await.unwrap().success());
        drop(tree);
        let storage = crate::database::Storage::new(
            &directory.join("settings.db"),
            &directory.join("legacy"),
        );
        storage
            .database()
            .unwrap()
            .save_tool(
                RequiredToolId::Ffmpeg,
                &RequiredToolConfig {
                    source: RequiredToolSource::Manual,
                    manual_path: ffmpeg.to_string_lossy().into_owned(),
                    checked_at: crate::datetime::now(),
                    programs: vec![
                        Program {
                            name: "ffmpeg".into(),
                            path: ffmpeg,
                            version: "7.1".into(),
                        },
                        Program {
                            name: "ffprobe".into(),
                            path: ffprobe,
                            version: "7.1".into(),
                        },
                    ],
                },
            )
            .unwrap();
        let mut context = tauri::generate_context!();
        context.config_mut().app.windows.clear();
        let app = tauri::Builder::default()
            .any_thread()
            .manage(AudioExtractionManager::default())
            .manage(RequiredToolManager::new(storage.clone()))
            .manage(storage)
            .build(context)
            .unwrap();
        let parsed =
            parse_audio_source(app.handle().clone(), source.to_string_lossy().into_owned())
                .await
                .unwrap();
        assert_eq!(parsed.phase, "ready");
        if let Some(path) = std::env::var_os("EVD_AUDIO_TEST_SNAPSHOT") {
            std::fs::write(path, serde_json::to_vec(&parsed).unwrap()).unwrap();
        }
        let info = parsed.info.unwrap();
        assert!(info.tracks.iter().all(|track| track.output_format == "M4A"));
        let error = extract_audio(app.handle().clone(), info.id.clone(), 100)
            .await
            .unwrap_err();
        assert_eq!(error.code, "invalidTrack");
        assert!(!app.state::<AudioExtractionManager>().is_active().unwrap());
        let finished = extract_audio(app.handle().clone(), info.id, info.default_track)
            .await
            .unwrap();
        assert_eq!(finished.phase, "completed");
        assert_eq!(finished.progress, Some(100.0));
        assert!(source.with_extension("m4a").is_file());
        let cleared = clear_audio_source(app.handle().clone(), app.state()).unwrap();
        assert_eq!(cleared.phase, "idle");
        assert!(cleared.info.is_none());
        let ui = crate::database::ui_preferences::UiPreferences {
            active_page: "audio".into(),
            ..Default::default()
        };
        app.state::<crate::database::Storage>()
            .database()
            .unwrap()
            .save_ui_preferences(&ui)
            .unwrap();
        assert_eq!(
            app.state::<crate::database::Storage>()
                .database()
                .unwrap()
                .ui_preferences()
                .unwrap()
                .active_page,
            "audio"
        );
    });
}
