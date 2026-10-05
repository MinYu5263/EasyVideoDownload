use super::*;
use serde_json::json;

#[test]
fn accepts_only_douyin_https_video_links() {
    assert_eq!(
        normalize_link("https://v.douyin.com/V4Jkr52cl90/")
            .unwrap()
            .host_str(),
        Some("v.douyin.com")
    );
    assert!(normalize_link("https://douyin.com.evil.test/video/123").is_err());
    assert!(normalize_link("http://www.douyin.com/video/123").is_err());
    assert!(normalize_link("https://user:secret@www.douyin.com/video/123").is_err());
}

#[test]
fn extracts_real_dimensions_and_keeps_codec_and_bitrate_variants() {
    let detail = json!({"aweme_detail": {"aweme_id":"7688950275142389027", "desc":"test", "video": {
    "duration":170133, "bit_rate":[
        {"gear_name":"low_720", "bit_rate":2220329, "FPS":30, "is_bytevc1":0,
         "play_addr":{"width":1920,"height":1080,"data_size":47219203,"url_list":["https://media.test/a","https://media.test/b"]}},
        {"gear_name":"high_1080", "bit_rate":801764, "FPS":30, "is_bytevc1":1,
         "play_addr":{"width":1920,"height":1080,"data_size":17050920,"url_list":["https://media.test/c"]}},
        {"bit_rate":2159535,"is_bytevc1":0,
         "play_addr":{"width":1920,"height":1080,"data_size":45926028,"url_list":["https://media.test/d"]}}
    ]}}});
    let parsed = serde_json::to_value(parse_detail(&detail).unwrap().video).unwrap();
    let formats = parsed["formats"].as_array().unwrap();
    assert_eq!(formats.len(), 3);
    assert_eq!(formats[0]["width"], 1920);
    assert_eq!(formats[0]["height"], 1080);
    assert_eq!(formats[0]["fileSize"], 47219203);
    assert_eq!(formats[0]["fps"].as_f64(), Some(30.0));
    assert_eq!(formats[0]["codec"], "h264");
    assert!(formats.iter().any(|f| f["codec"] == "hevc"));
    assert!(!parsed.to_string().contains("media.test"));
}

#[test]
fn rejects_empty_or_non_video_detail() {
    assert!(parse_detail(&json!({})).is_err());
    assert!(parse_detail(&json!({"aweme_detail":{"aweme_id":"1","images":[{}]}})).is_err());
}

#[test]
fn boolean_hevc_flag_does_not_mislabel_the_codec() {
    let parsed = parse_detail(&json!({"aweme_detail":{"aweme_id":"1","video":{"bit_rate":[{"is_bytevc1":true,"play_addr":{"width":1280,"height":720,"url_list":["https://media.test/a"]}}]}}})).unwrap();
    assert_eq!(parsed.video.formats[0].codec, "hevc");
}

#[tokio::test]
#[ignore = "requires user-provided Cookie file, ffprobe and live Douyin network"]
async fn live_native_parser_and_download_verify_real_1080p() {
    let cookie_path = std::env::var("DOUYIN_LAB_COOKIE_FILE").expect("Cookie file env required");
    let probe = std::path::PathBuf::from(
        std::env::var("DOUYIN_LAB_FFPROBE").expect("ffprobe path env required"),
    );
    let contents = std::fs::read_to_string(cookie_path).unwrap();
    let client = super::http::client(&contents, None).unwrap();
    let parsed = super::http::parse_video(&client, "https://v.douyin.com/V4Jkr52cl90/")
        .await
        .unwrap();
    eprintln!(
        "Live candidates: {} formats, {} 1080p H264",
        parsed.video.formats.len(),
        parsed
            .video
            .formats
            .iter()
            .filter(|f| f.height == Some(1080) && f.codec == "h264")
            .count()
    );
    let candidate = parsed
        .candidates
        .iter()
        .find(|c| {
            c.format.height == Some(1080)
                && c.format.codec == "h264"
                && c.format.watermarked != Some(true)
        })
        .expect("Expected sample 1080p H264");
    let root = tempfile::Builder::new()
        .prefix("douyin-live-")
        .tempdir_in(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/douyin-lab-work"),
        )
        .unwrap();
    eprintln!("Live test owned directory: {}", root.path().display());
    let (path, media) = super::download::download(
        &client,
        &parsed,
        candidate,
        root.path(),
        root.path(),
        &probe,
        &super::cancel::Cancellation::default(),
        |_, _| {},
        || {},
    )
        .await
        .unwrap();
    eprintln!(
        "Live verified media: {}",
        serde_json::to_string(&media).unwrap()
    );
    assert_eq!((media.width, media.height), (1920, 1080));
    assert_eq!(media.codec, "h264");
    assert!(std::path::Path::new(&path).is_file());
    // TempDir owns exactly this test's output; no shared output directory cleanup.
    root.close().unwrap();
}
