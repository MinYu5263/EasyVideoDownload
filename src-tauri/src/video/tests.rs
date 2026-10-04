use super::*;
use serde_json::json;

#[test]
fn extracts_douyin_share_text_and_accepts_other_platform_links() {
    assert_eq!(
        normalize_link(
            "分享 https://v.douyin.com/abc/，复制打开",
            CookiePlatform::Douyin
        )
        .unwrap(),
        "https://v.douyin.com/abc/"
    );
    assert_eq!(
        normalize_link("https://b23.tv/abc", CookiePlatform::Bilibili).unwrap(),
        "https://b23.tv/abc"
    );
    assert_eq!(
        normalize_link("https://youtu.be/abc", CookiePlatform::Youtube).unwrap(),
        "https://youtu.be/abc"
    );
}

#[test]
fn rejects_wrong_hosts_credentials_and_protocols() {
    for input in [
        "hello",
        "ftp://douyin.com/a",
        "https://secret@douyin.com/a",
        "https://douyin.com.attacker.com/a",
        "https://youtube.com/a",
    ] {
        assert!(
            normalize_link(input, CookiePlatform::Douyin).is_err(),
            "{input}"
        );
    }
}

fn sample() -> serde_json::Value {
    json!({"id": "123", "title": "Real title", "thumbnail": "https://example.com/cover.jpg", "duration": 12.5, "ext": "mp4",
    "formats": [
        {"format_id": "video", "vcodec": "h264", "height": 1080, "fps": 60, "ext": "mp4"},
        {"format_id": "audio", "vcodec": "none"},
        {"format_id": "storyboard", "vcodec": "none", "protocol": "mhtml"},
        {"format_id": "drm", "vcodec": "h264", "has_drm": true}
    ]})
}

#[test]
fn metadata_uses_real_fields_and_excludes_audio_storyboards_and_drm() {
    let video = parse_metadata(&serde_json::to_vec(&sample()).unwrap()).unwrap();
    assert_eq!(video.title, "Real title");
    assert_eq!(video.duration, Some(12.5));
    assert_eq!(video.extension.as_deref(), Some("mp4"));
    assert_eq!(video.formats.len(), 1);
    assert_eq!(video.formats[0].height, Some(1080));
    assert_eq!(video.formats[0].fps, Some(60.0));
    assert_eq!(video.formats[0].extension.as_deref(), Some("mp4"));
}

#[test]
fn missing_optional_metadata_is_not_fabricated() {
    let video = parse_metadata(
        br#"{"id":"1","title":"Title","formats":[{"format_id":"1","vcodec":"h264"}]}"#,
    )
    .unwrap();
    assert!(video.duration.is_none());
    assert!(video.thumbnail.is_none());
    assert!(video.extension.is_none());
    assert!(video.formats[0].height.is_none());
    assert!(video.formats[0].fps.is_none());
}

#[test]
fn zero_duration_metadata_can_be_persisted_as_unknown() {
    let mut source = sample();
    source["duration"] = json!(0);
    let video = parse_metadata(&serde_json::to_vec(&source).unwrap()).unwrap();
    assert_eq!(video.duration, None);
    let dir = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary duration test directory: {}",
        dir.path().display()
    );
    let db =
        crate::database::Database::open(&dir.path().join("app.db"), &dir.path().join("legacy"))
            .unwrap();
    let mut page = crate::database::persistence_tests::page();
    page.duration_seconds = video.duration;
    db.save_download_page_state(&page).unwrap();
}

#[test]
fn metadata_returns_exact_estimated_and_unknown_sizes_for_each_video_stream() {
    let mut source = sample();
    source["formats"] = json!([
        {"format_id": "exact", "filesize": 10485760, "filesize_approx": 12000000},
        {"format_id": "estimate", "filesize_approx": 31457280.4},
        {"format_id": "unknown"},
        {"format_id": "invalid", "filesize": -3, "filesize_approx": "2048"},
        {"format_id": "zero", "filesize": 0},
        {"format_id": "fallback", "filesize": -1, "filesize_approx": 2048},
        {"format_id": "too-large", "filesize": 9007199254740992_u64}
    ]);
    let video = parse_metadata(&serde_json::to_vec(&source).unwrap()).unwrap();
    let payload = serde_json::to_value(video).unwrap();
    for (index, (bytes, approximate)) in [
        (Some(10485760_u64), false),
        (Some(31457280), true),
        (None, false),
        (None, false),
        (None, false),
        (Some(2048), true),
        (None, false),
    ]
    .into_iter()
    .enumerate()
    {
        let format = &payload["formats"][index];
        assert!(
            format.get("sizeBytes").is_some(),
            "Native payload must include sizeBytes"
        );
        assert_eq!(format["sizeBytes"].as_u64(), bytes, "{format}");
        assert_eq!(
            format["sizeApproximate"].as_bool(),
            Some(approximate),
            "{format}"
        );
    }
}

#[test]
fn bilibili_flv_with_unknown_codec_remains_available() {
    // yt-dlp's durl extractor returns FLV height/URL without vcodec or fps.
    let video = parse_metadata(br#"{"id":"BV1","title":"FLV title","formats":[{"format_id":"80","height":1080,"ext":"flv","protocol":"https","url":"https://example.com/video.flv"}]}"#).unwrap();
    assert_eq!(video.formats.len(), 1);
    assert_eq!(video.formats[0].height, Some(1080));
    assert!(video.formats[0].fps.is_none());
}

#[test]
fn malformed_metadata_and_no_playable_formats_are_errors() {
    assert_eq!(
        parse_metadata(b"not json").unwrap_err().code,
        "invalidResult"
    );
    assert_eq!(
        parse_metadata(br#"{"id":"1","title":"Title","formats":[]}"#)
            .unwrap_err()
            .code,
        "noFormats"
    );
    assert_eq!(
        parse_metadata(br#"{"title":"Title"}"#).unwrap_err().code,
        "invalidResult"
    );
}

#[test]
fn collections_and_segmented_videos_cannot_masquerade_as_a_complete_single_video() {
    let single = json!({"_type": "multi_video", "entries": [sample()]});
    assert_eq!(
        parse_metadata(&serde_json::to_vec(&single).unwrap())
            .unwrap_err()
            .code,
        "unsupportedVideo"
    );
    let multiple = json!({"_type": "playlist", "entries": [sample(), sample()]});
    assert_eq!(
        parse_metadata(&serde_json::to_vec(&multiple).unwrap())
            .unwrap_err()
            .code,
        "unsupportedVideo"
    );
    let selected = json!({"_type": "playlist", "playlist_count": 3, "entries": [sample()]});
    assert_eq!(
        parse_metadata(&serde_json::to_vec(&selected).unwrap())
            .unwrap_err()
            .code,
        "unsupportedVideo"
    );
}

#[tokio::test]
async fn youtube_reload_with_cookies_retries_once_as_public_and_marks_the_result() {
    let mut calls = Vec::new();
    let result = parse_with_cookie_retry(CookiePlatform::Youtube, true, |cookie| {
        calls.push(cookie);
        async move {
            if cookie {
                Err(error("youtubeReloadRequired", "page needs reload"))
            } else {
                parse_metadata(&serde_json::to_vec(&sample()).unwrap())
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(calls, [true, false]);
    assert!(result.cookie_fallback);
    assert_eq!(result.title, "Real title");
}

#[tokio::test]
async fn youtube_public_retry_is_limited_to_the_exact_cookie_reload_failure() {
    for (platform, has_cookie, code) in [
        (CookiePlatform::Douyin, true, "youtubeReloadRequired"),
        (CookiePlatform::Bilibili, true, "youtubeReloadRequired"),
        (CookiePlatform::Youtube, false, "youtubeReloadRequired"),
        (CookiePlatform::Youtube, true, "timeout"),
        (CookiePlatform::Youtube, true, "cookieReadFailed"),
    ] {
        let mut calls = Vec::new();
        let failure = parse_with_cookie_retry(platform, has_cookie, |cookie| {
            calls.push(cookie);
            async move { Err(error(code, "actual failure")) }
        })
        .await
        .unwrap_err();
        assert_eq!(calls, [has_cookie]);
        assert_eq!(failure.code, code);
    }
}

#[tokio::test]
async fn authenticated_success_keeps_its_formats_and_public_retry_failure_is_not_success() {
    let mut calls = Vec::new();
    let result = parse_with_cookie_retry(CookiePlatform::Youtube, true, |cookie| {
        calls.push(cookie);
        async { parse_metadata(&serde_json::to_vec(&sample()).unwrap()) }
    })
    .await
    .unwrap();
    assert_eq!(calls, [true]);
    assert!(!result.cookie_fallback);

    let mut calls = Vec::new();
    let failure = parse_with_cookie_retry(CookiePlatform::Youtube, true, |cookie| {
        calls.push(cookie);
        async move {
            Err(if cookie {
                error("youtubeReloadRequired", "reload")
            } else {
                error("parseFailed", "Authentication required")
            })
        }
    })
    .await
    .unwrap_err();
    assert_eq!(calls, [true, false]);
    assert_eq!(failure.code, "parseFailed");
    assert!(failure.detail.contains("Authentication required"));
}

fn tools(path: std::path::PathBuf, deno: Option<std::path::PathBuf>) -> RequiredToolSettings {
    use crate::required_tools::{Program, RequiredToolConfig, RequiredToolSource};
    let mut settings = RequiredToolSettings::default();
    for (id, name, path) in [
        (RequiredToolId::Ytdlp, "yt-dlp", Some(path)),
        (RequiredToolId::Deno, "deno", deno),
    ] {
        if let Some(path) = path {
            settings.tools.insert(
                id,
                RequiredToolConfig {
                    source: RequiredToolSource::Manual,
                    manual_path: path.to_string_lossy().into(),
                    programs: vec![Program {
                        name: name.into(),
                        path,
                        version: "test".into(),
                    }],
                    checked_at: "2026-10-03T00:00:00Z".into(),
                },
            );
        }
    }
    settings
}

#[test]
fn displayed_commands_quote_paths_and_arguments_for_each_shell() {
    let mut command = Command::new("C:/Tool dir/O'Brien/yt-dlp.exe");
    command.args([
        "--simulate",
        "--",
        "https://youtu.be/abc?x=$HOME&y=`echo`&z='",
    ]);
    let windows = render_command(&command, true).unwrap();
    assert_eq!(windows.shell, "PowerShell");
    assert_eq!(windows.text, "& 'C:/Tool dir/O''Brien/yt-dlp.exe' '--simulate' '--' 'https://youtu.be/abc?x=$HOME&y=`echo`&z='''");
    let unix = render_command(&command, false).unwrap();
    assert_eq!(unix.shell, "Shell");
    assert_eq!(unix.text, "'C:/Tool dir/O'\"'\"'Brien/yt-dlp.exe' '--simulate' '--' 'https://youtu.be/abc?x=$HOME&y=`echo`&z='\"'\"''");
}

#[test]
fn command_preview_uses_shared_parser_options_and_managed_cookie_path_without_writing() {
    let directory = tempfile::tempdir().unwrap();
    println!(
        "command preview test directory: {}",
        directory.path().display()
    );
    let store = CookieStore::new(directory.path());
    let secret = "cookie-secret-must-not-appear";
    store.save(CookiePlatform::Douyin, secret).unwrap();
    let settings = tools(
        "C:/Tool dir/yt-dlp.exe".into(),
        Some("C:/Tool dir/deno.exe".into()),
    );
    let preview = command_preview(
        &settings,
        &store,
        CookiePlatform::Douyin,
        "分享 https://v.douyin.com/abc/，复制打开抖音",
        None,
    )
    .unwrap();
    let path = store.path(CookiePlatform::Douyin);
    let actual =
        parsing_command(&settings, "https://v.douyin.com/abc/", Some(&path), None).unwrap();
    assert_eq!(
        preview.text,
        render_command(&actual, cfg!(windows)).unwrap().text
    );
    assert!(preview.text.contains("douyin_cookies.txt"));
    assert!(preview.text.contains("--simulate"));
    assert!(!preview.text.contains(secret));
    assert_eq!(store.load(CookiePlatform::Douyin).unwrap(), secret);
    assert_eq!(
        std::fs::read_dir(directory.path().join("cookies"))
            .unwrap()
            .count(),
        1
    );
    let public = command_preview(
        &settings,
        &store,
        CookiePlatform::Youtube,
        "https://youtu.be/abc",
        None,
    )
    .unwrap();
    assert!(!public.text.contains("--cookies"));
    assert!(!store.path(CookiePlatform::Youtube).exists());
}

#[test]
fn command_preview_rejects_invalid_links_missing_tools_and_unreadable_cookies() {
    let directory = tempfile::tempdir().unwrap();
    println!(
        "command preview error test directory: {}",
        directory.path().display()
    );
    let store = CookieStore::new(directory.path());
    let settings = tools("yt-dlp.exe".into(), None);
    assert_eq!(
        command_preview(
            &settings,
            &store,
            CookiePlatform::Youtube,
            "https://douyin.com/video/1",
            None
        )
        .unwrap_err()
        .code,
        "platformMismatch"
    );
    assert_eq!(
        command_preview(
            &RequiredToolSettings::default(),
            &store,
            CookiePlatform::Youtube,
            "https://youtu.be/abc",
            None
        )
        .unwrap_err()
        .code,
        "toolMissing"
    );
    std::fs::create_dir_all(store.path(CookiePlatform::Youtube)).unwrap();
    assert_eq!(
        command_preview(
            &settings,
            &store,
            CookiePlatform::Youtube,
            "https://youtu.be/abc",
            None
        )
        .unwrap_err()
        .code,
        "cookieReadFailed"
    );
}

fn download_tools() -> RequiredToolSettings {
    let mut settings = tools(
        "C:/Tools/yt-dlp.exe".into(),
        Some("C:/Tools/deno.exe".into()),
    );
    let mut ffmpeg = settings.tools[&RequiredToolId::Ytdlp].clone();
    ffmpeg.programs[0].name = "ffmpeg".into();
    ffmpeg.programs[0].path = "C:/Tools/ffmpeg.exe".into();
    settings.tools.insert(RequiredToolId::Ffmpeg, ffmpeg);
    settings
}

#[test]
fn parsing_downloads_and_previews_share_explicit_proxy_settings() {
    let directory = tempfile::tempdir().unwrap();
    eprintln!(
        "proxy command test directory: {}",
        directory.path().display()
    );
    let store = CookieStore::new(directory.path());
    let settings = download_tools();
    let options: DownloadCommandOptions = serde_json::from_value(json!({
        "directory": directory.path(), "formatId": "399"
    }))
        .unwrap();
    let proxy = crate::proxy::ProxySettings {
        protocol: "socks5".into(),
        address: "::1".into(),
        port: 7890,
    };
    for (proxy, expected) in [(Some(&proxy), "socks5h://[::1]:7890"), (None, "")] {
        for command in [
            parsing_command(&settings, "https://youtu.be/abc", None, proxy).unwrap(),
            download_command(&settings, "https://youtu.be/abc", None, &options, proxy).unwrap(),
        ] {
            let args: Vec<_> = command
                .as_std()
                .get_args()
                .map(|arg| arg.to_string_lossy())
                .collect();
            assert_eq!(
                args.iter().filter(|arg| arg.starts_with("--proxy")).count(),
                usize::from(proxy.is_some())
            );
            if proxy.is_some() {
                let position = args.iter().position(|arg| arg == "--proxy").unwrap();
                assert_eq!(args[position + 1], expected);
                assert!(position < args.iter().position(|arg| arg == "--").unwrap());
                let env: std::collections::BTreeMap<_, _> = command.as_std().get_envs().collect();
                assert_eq!(env.get(std::ffi::OsStr::new("NO_PROXY")), Some(&None));
                assert_eq!(
                    env.get(std::ffi::OsStr::new("HTTP_PROXY")),
                    Some(&Some(std::ffi::OsStr::new(expected)))
                );
            } else {
                assert_eq!(
                    command.as_std().get_envs().count(),
                    0,
                    "disabling the app proxy must preserve the original network environment"
                );
            }
        }
        for preview in [
            command_preview(
                &settings,
                &store,
                CookiePlatform::Youtube,
                "https://youtu.be/abc",
                proxy,
            )
                .unwrap(),
            download_command_preview(
                &settings,
                &store,
                CookiePlatform::Youtube,
                "https://youtu.be/abc",
                &options,
                proxy,
            )
                .unwrap(),
        ] {
            assert!(!preview.text.contains("SetEnvironmentVariable"));
            assert!(!preview.text.contains("NO_PROXY"));
            assert!(!preview.text.contains("ALL_PROXY"));
            if proxy.is_some() {
                assert!(preview.text.contains(&format!("'--proxy' '{expected}'")));
            } else {
                assert!(!preview.text.contains("--proxy"));
            }
        }
    }
}

#[test]
fn public_download_preview_does_not_read_or_pass_cookies() {
    let directory = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary public download preview directory: {}",
        directory.path().display()
    );
    let store = CookieStore::new(directory.path());
    std::fs::create_dir_all(store.path(CookiePlatform::Youtube)).unwrap();
    let options: DownloadCommandOptions = serde_json::from_value(json!({
        "directory": directory.path(), "formatId": "399", "cookieFallback": true
    }))
    .unwrap();
    let preview = download_command_preview(
        &download_tools(),
        &store,
        CookiePlatform::Youtube,
        "https://youtu.be/abc",
        &options,
        None,
    )
    .unwrap();
    assert!(!preview.text.contains("--cookies"));
}

#[test]
fn download_preview_uses_the_selected_native_format_without_conversion_or_writing_files() {
    let directory = tempfile::tempdir().unwrap();
    println!(
        "download preview test directory: {}",
        directory.path().display()
    );
    let output = directory.path().join("video output");
    let options: DownloadCommandOptions = serde_json::from_value(json!({
        "directory": output, "formatId": "399"
    }))
    .unwrap();
    let settings = download_tools();
    let command =
        download_command(&settings, "https://youtu.be/abc", None, &options, None).unwrap();
    let args: Vec<_> = command
        .as_std()
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--paths", output.to_str().unwrap()]));
    assert!(args.windows(2).any(|pair| pair
        == [
            "--format",
            "bestvideo[format_id=\"399\"]+bestaudio/best*[format_id=\"399\"]"
        ]));
    assert!(!args.iter().any(|arg| arg == "--merge-output-format"
        || arg == "--remux-video"
        || arg == "--recode-video"));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--ffmpeg-location", "C:/Tools/ffmpeg.exe"]));
    assert!(!args
        .iter()
        .any(|arg| arg == "--simulate" || arg == "--dump-single-json"));
    assert!(args.iter().any(|arg| arg == "--progress"));
    assert!(args.windows(2).any(|pair| pair
        == [
            "--print",
            "after_move:__EVD_FILE__%(.{filepath,__real_download})j"
        ]));
    let store = CookieStore::new(directory.path());
    store
        .save(CookiePlatform::Youtube, "private-cookie-value")
        .unwrap();
    let preview = download_command_preview(
        &settings,
        &store,
        CookiePlatform::Youtube,
        "https://youtu.be/abc",
        &options,
        None,
    )
    .unwrap();
    assert!(preview.text.contains("youtube_cookies.txt"));
    assert!(!preview.text.contains("private-cookie-value"));
    assert!(!output.exists());
    assert_eq!(
        store.load(CookiePlatform::Youtube).unwrap(),
        "private-cookie-value"
    );
}

#[test]
fn download_command_preserves_mp4_without_overriding_automatic_audio_selection() {
    let directory = tempfile::tempdir().unwrap();
    eprintln!("container test directory: {}", directory.path().display());
    let settings = download_tools();
    {
        let (container, selector) = (
            "mp4",
            "bestvideo[format_id=\"616\"]+bestaudio/best*[format_id=\"616\"]",
        );
        let options: DownloadCommandOptions = serde_json::from_value(json!({
            "directory": directory.path(), "formatId": "616", "container": container
        }))
        .unwrap();
        let command =
            download_command(&settings, "https://youtu.be/abc", None, &options, None).unwrap();
        let args: Vec<_> = command
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--merge-output-format", container]),
            "{container}"
        );
        assert!(
            args.windows(2).any(|pair| pair == ["--format", selector]),
            "{container}"
        );
        assert!(!args
            .iter()
            .any(|arg| arg == "--recode-video" || arg == "--remux-video"));
    }
    for container in ["webm", "mp4/mkv"] {
        let invalid: DownloadCommandOptions = serde_json::from_value(json!({
            "directory": directory.path(), "formatId": "616", "container": container
        }))
        .unwrap();
        assert_eq!(
            download_command(&settings, "https://youtu.be/abc", None, &invalid, None)
                .unwrap_err()
                .code,
            "invalidDownloadOptions"
        );
    }
}

#[test]
fn download_preview_validates_native_format_id_and_directory() {
    let directory = tempfile::tempdir().unwrap();
    println!(
        "download options test directory: {}",
        directory.path().display()
    );
    let settings = download_tools();
    let options: DownloadCommandOptions = serde_json::from_value(json!({
        "directory": directory.path(), "formatId": "dash-flv_1080"
    }))
    .unwrap();
    let command =
        download_command(&settings, "https://youtu.be/abc", None, &options, None).unwrap();
    let args: Vec<_> = command
        .as_std()
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert!(args.windows(2).any(|pair| pair
        == [
            "--format",
            "bestvideo[format_id=\"dash-flv_1080\"]+bestaudio/best*[format_id=\"dash-flv_1080\"]"
        ]));
    assert!(!args
        .iter()
        .any(|arg| arg == "--merge-output-format" || arg == "--remux-video"));
    let relative: DownloadCommandOptions = serde_json::from_value(json!({
        "directory": "relative", "formatId": "399"
    }))
    .unwrap();
    assert_eq!(
        download_command(&settings, "https://youtu.be/abc", None, &relative, None)
            .unwrap_err()
            .code,
        "invalidDownloadDirectory"
    );
    for format_id in ["", "399]+bestaudio", "399\"", "399\n", "a/b", "a,b"] {
        let invalid: DownloadCommandOptions = serde_json::from_value(json!({
            "directory": directory.path(), "formatId": format_id
        }))
        .unwrap();
        assert_eq!(
            download_command(&settings, "https://youtu.be/abc", None, &invalid, None)
                .unwrap_err()
                .code,
            "invalidDownloadOptions"
        );
    }
    assert_eq!(
        download_command(
            &tools("yt-dlp.exe".into(), None),
            "https://youtu.be/abc",
            None,
            &options,
            None
        )
        .unwrap_err()
        .code,
        "ffmpegMissing"
    );
}

#[test]
fn parsing_uses_the_configured_program_and_runtime_and_never_downloads() {
    let settings = tools(
        std::path::PathBuf::from("/configured/yt-dlp"),
        Some("/configured/deno".into()),
    );
    let cookie = Path::new("/selected/cookies.txt");
    let command = parsing_command(&settings, "https://youtu.be/abc", Some(cookie), None).unwrap();
    let command = command.as_std();
    assert_eq!(command.get_program(), "/configured/yt-dlp");
    let arguments: Vec<_> = command
        .get_args()
        .map(|arg| arg.to_string_lossy())
        .collect();
    for flag in [
        "--simulate",
        "--ignore-config",
        "--no-plugin-dirs",
        "--dump-single-json",
        "--no-playlist",
        "--no-cache-dir",
    ] {
        assert!(
            arguments.iter().any(|argument| argument == flag),
            "missing {flag}"
        );
    }
    assert!(arguments
        .windows(2)
        .any(|pair| pair == ["--cookies", "/selected/cookies.txt"]));
    assert!(arguments
        .windows(2)
        .any(|pair| pair == ["--js-runtimes", "deno:/configured/deno"]));
    assert_eq!(arguments.last().unwrap(), "https://youtu.be/abc");
    assert_eq!(
        parsing_command(
            &RequiredToolSettings::default(),
            "https://youtu.be/abc",
            None,
            None
        )
        .unwrap_err()
        .code,
        "toolMissing"
    );
}

#[test]
fn cookie_snapshot_uses_only_selected_platform_and_cannot_overwrite_its_file() {
    let directory = tempfile::Builder::new()
        .prefix("evd-video-test-")
        .tempdir()
        .unwrap();
    eprintln!("temporary test directory: {}", directory.path().display());
    let store = CookieStore::new(directory.path());
    store
        .save(CookiePlatform::Douyin, "selected raw contents")
        .unwrap();
    store
        .save(CookiePlatform::Youtube, "other platform")
        .unwrap();
    let copy = cookie_snapshot(&store, CookiePlatform::Douyin)
        .unwrap()
        .unwrap();
    eprintln!("temporary Cookie snapshot: {}", copy.path().display());
    let path = copy.path().to_path_buf();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "selected raw contents"
    );
    std::fs::write(&path, "yt-dlp changed its jar").unwrap();
    assert_eq!(
        store.load(CookiePlatform::Douyin).unwrap(),
        "selected raw contents"
    );
    assert!(cookie_snapshot(&store, CookiePlatform::Bilibili)
        .unwrap()
        .is_none());
    drop(copy);
    assert!(!path.exists());
}

fn fixture(mode: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args([
        "--ignored",
        "--exact",
        "video::tests::process_fixture",
        "--nocapture",
    ]);
    command.env("EVD_VIDEO_TEST_PROCESS", mode);
    command
}

#[cfg(windows)]
#[tokio::test]
async fn parse_command_blocks_tool_publication_until_native_success_or_failure() {
    let directory = tempfile::Builder::new()
        .prefix("evd-parse-tool-usage-")
        .tempdir()
        .unwrap();
    eprintln!(
        "temporary parser usage directory: {}",
        directory.path().display()
    );
    let mut command = fixture("toolUsage");
    command.env("EVD_VIDEO_TEST_USAGE_DIRECTORY", directory.path());
    collect_metadata(command, Duration::from_secs(10))
        .await
        .unwrap();
    directory.close().unwrap();
}

#[cfg(windows)]
async fn parse_command_tool_usage_fixture(directory: &Path) {
    use tauri::Manager;
    let parser = directory.join("yt-dlp.cmd");
    std::fs::write(
        &parser,
        r#"@echo off
echo ready > "%~dp0ready"
:wait
if not exist "%~dp0release" goto wait
if exist "%~dp0failure" (
  echo video unavailable 1>&2
  exit /b 3
)
echo {"id":"123","title":"Native result","formats":[{"format_id":"video","vcodec":"h264"}]}
"#,
    )
        .unwrap();
    let storage = Storage::new(&directory.join("app.db"), &directory.join("legacy"));
    let mut settings = tools(parser, None);
    let config = settings.tools.get_mut(&RequiredToolId::Ytdlp).unwrap();
    config.programs[0].version = "2026.10.04".into();
    config.checked_at = crate::datetime::now();
    storage
        .database()
        .unwrap()
        .save_tool(RequiredToolId::Ytdlp, config)
        .unwrap();
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    let app = tauri::Builder::default()
        .any_thread()
        .manage(RequiredToolManager::new(storage.clone()))
        .manage(CookieStore::new(directory))
        .manage(storage)
        .build(context)
        .unwrap();
    for failure in [false, true] {
        let ready = directory.join("ready");
        let release = directory.join("release");
        if failure {
            std::fs::remove_file(&ready).unwrap();
            std::fs::remove_file(&release).unwrap();
            std::fs::write(directory.join("failure"), "fail").unwrap();
        }
        let tools = app.state::<RequiredToolManager>();
        let observe_usage = async {
            tokio::time::timeout(Duration::from_secs(5), async {
                while !ready.exists() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
                .await
                .unwrap();
            let blocked = tools.usage.publication(directory).is_err();
            std::fs::write(&release, "finish").unwrap();
            blocked
        };
        let (result, blocked) = tokio::join!(
            parse_video(
                CookiePlatform::Douyin,
                "https://v.douyin.com/abc/".into(),
                app.state(),
                app.state(),
                app.state()
            ),
            observe_usage
        );
        if failure {
            assert_eq!(result.unwrap_err().code, "parseFailed");
        } else {
            assert_eq!(result.unwrap().metadata.title, "Native result");
        }
        assert!(blocked, "native parsing must prevent replacing its tools");
        assert!(
            tools.usage.publication(directory).is_ok(),
            "finished parsing retained its tool lease"
        );
    }
}

#[test]
#[ignore = "native child fixture, invoked only by process tests"]
fn process_fixture() {
    match std::env::var("EVD_VIDEO_TEST_PROCESS").unwrap().as_str() {
        #[cfg(windows)]
        "toolUsage" => {
            let directory = std::path::PathBuf::from(
                std::env::var_os("EVD_VIDEO_TEST_USAGE_DIRECTORY").unwrap(),
            );
            tauri::async_runtime::block_on(parse_command_tool_usage_fixture(&directory));
        }
        "sleep" => {
            std::thread::sleep(Duration::from_secs(2));
            std::fs::write(
                std::env::var("EVD_VIDEO_TEST_MARKER").unwrap(),
                "still alive",
            )
            .unwrap();
        }
        "exit" => {
            eprintln!("video unavailable");
            std::process::exit(3);
        }
        "cookie" => {
            eprintln!("failed to load cookies: very-secret-cookie-value");
            std::process::exit(3);
        }
        "youtubeReload" => {
            eprintln!("ERROR: [youtube] 9lU0O9352ms: The page needs to be reloaded.");
            std::process::exit(3);
        }
        "flood" => {
            eprintln!("{}", "x".repeat(70_000));
        }
        "success" => {
            println!("{}", sample());
        }
        _ => panic!("unknown fixture"),
    }
}

#[tokio::test]
async fn native_output_and_failures_are_observed_instead_of_simulated_success() {
    let output = collect_metadata(fixture("success"), Duration::from_secs(5))
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&output).contains("Real title"));
    let failure = collect_metadata(fixture("exit"), Duration::from_secs(5))
        .await
        .unwrap_err();
    assert_eq!(failure.code, "parseFailed");
    assert!(failure.detail.contains("video unavailable"));
    let failure = collect_metadata(fixture("cookie"), Duration::from_secs(5))
        .await
        .unwrap_err();
    assert!(!failure.detail.contains("very-secret-cookie-value"));
    assert_eq!(
        collect_metadata(fixture("flood"), Duration::from_secs(5))
            .await
            .unwrap_err()
            .code,
        "outputTooLarge"
    );
}

#[tokio::test]
async fn reload_error_from_native_ytdlp_is_identified_without_exposing_cookie_contents() {
    let failure = collect_metadata(fixture("youtubeReload"), Duration::from_secs(5))
        .await
        .unwrap_err();
    assert_eq!(failure.code, "youtubeReloadRequired");
}

#[tokio::test]
async fn native_parser_timeout_stops_the_process() {
    let directory = tempfile::Builder::new()
        .prefix("evd-video-timeout-")
        .tempdir()
        .unwrap();
    eprintln!("temporary test directory: {}", directory.path().display());
    let marker = directory.path().join("must-not-exist");
    let mut command = fixture("sleep");
    command.env("EVD_VIDEO_TEST_MARKER", &marker);
    assert_eq!(
        collect_metadata(command, Duration::from_millis(150))
            .await
            .unwrap_err()
            .code,
        "timeout"
    );
    tokio::time::sleep(Duration::from_millis(2200)).await;
    assert!(!marker.exists(), "parser left a timed-out process running");
}

#[tokio::test]
#[ignore = "explicit live parsing smoke test; needs EVD_LIVE_URL and installed yt-dlp"]
async fn live_parse() {
    let input = std::env::var("EVD_LIVE_URL").expect("set EVD_LIVE_URL");
    let platform = match std::env::var("EVD_LIVE_PLATFORM")
        .expect("set EVD_LIVE_PLATFORM")
        .as_str()
    {
        "douyin" => CookiePlatform::Douyin,
        "bilibili" => CookiePlatform::Bilibili,
        "youtube" => CookiePlatform::Youtube,
        _ => panic!("unknown platform"),
    };
    let path = std::env::var_os("EVD_LIVE_YTDLP")
        .map(Into::into)
        .unwrap_or_else(|| which::which("yt-dlp").unwrap());
    let deno = which::which("deno").ok();
    let directory = tempfile::Builder::new()
        .prefix("evd-live-parse-")
        .tempdir()
        .unwrap();
    eprintln!(
        "temporary live test directory: {}",
        directory.path().display()
    );
    let source = std::env::var_os("EVD_LIVE_DATA_DIRECTORY")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| directory.path().to_path_buf());
    let store = CookieStore::new(&source);
    let original_cookie = std::fs::read(store.path(platform)).ok();
    let settings = tools(path, deno);
    let result = if let Ok(client) = std::env::var("EVD_LIVE_YOUTUBE_CLIENT") {
        let url = normalize_link(&input, platform).unwrap();
        let copy = cookie_snapshot(&store, platform).unwrap();
        if let Some(file) = &copy {
            eprintln!("temporary Cookie snapshot: {}", file.path().display());
        }
        let base =
            parsing_command(&settings, &url, copy.as_ref().map(|file| file.path()), None).unwrap();
        let args: Vec<_> = base
            .as_std()
            .get_args()
            .map(std::ffi::OsStr::to_os_string)
            .collect();
        let mut command = Command::new(base.as_std().get_program());
        command
            .args(&args[..args.len() - 2])
            .arg("--extractor-args")
            .arg(format!("youtube:player_client={client}"))
            .args(&args[args.len() - 2..]);
        collect_metadata(command, Duration::from_secs(90))
            .await
            .and_then(|bytes| parse_metadata(&bytes))
    } else {
        parse_with_tools(settings, store.clone(), platform, &input, None).await
    };
    assert!(
        std::fs::read(store.path(platform)).ok() == original_cookie,
        "live parsing changed the managed Cookie file"
    );
    match result {
        Ok(video) => {
            assert!(
                std::env::var("EVD_LIVE_EXPECT_ERROR").is_err(),
                "expected an error but metadata was returned"
            );
            if let Ok(expected) = std::env::var("EVD_LIVE_EXPECT_FALLBACK") {
                assert_eq!(video.cookie_fallback, expected == "true");
            }
            eprintln!(
                "native parse succeeded: id={}, title={}, duration={:?}, formats={}, cookie_fallback={}",
                video.id,
                video.title,
                video.duration,
                video.formats.len(),
                video.cookie_fallback
            );
        }
        Err(failure) => {
            if let Ok(expected) = std::env::var("EVD_LIVE_EXPECT_ERROR") {
                assert_eq!(failure.code, expected);
                eprintln!(
                    "native parser returned the expected {}: {}",
                    failure.code, failure.detail
                );
                return;
            }
            panic!("native parse returned {}: {}", failure.code, failure.detail);
        }
    }
}
