use super::VideoMetadata;
use crate::app_logs::{entry, LogEntry};
use std::time::Duration;

// Keep each event below the shared logger's message limit. Never serialize the
// complete metadata: it also contains titles, cover URLs and native cache keys.
pub(super) fn text(value: &str) -> String {
    if value.contains("://") || value.to_ascii_lowercase().contains("cookie") {
        return "[redacted]".into();
    }
    crate::app_logs::safe_text(value)
        .chars()
        .filter(|c| !c.is_control())
        .take(96)
        .collect()
}

fn unique(values: impl Iterator<Item=String>) -> Vec<String> {
    let mut result = Vec::new();
    for value in values {
        if !result.contains(&value) {
            result.push(value);
        }
    }
    result
}

// Log individual proxy fields so the shared URL redaction does not hide the
// endpoint. Never dump process arguments, environment variables or HTTP headers.
pub(super) fn network_summary(platform: &str, proxy: Option<&crate::proxy::ProxySettings>) -> String {
    let native = platform == "douyin";
    serde_json::json!({
        "engine": if native { "native-http" } else { "yt-dlp" },
        "proxyEnabled": proxy.is_some(),
        "proxyMode": if proxy.is_some() { "explicit" } else if native { "direct" } else { "system" },
        "proxyArgument": (!native && proxy.is_some()).then_some("--proxy"),
        "proxyProtocol": proxy.map(|p| if p.protocol == "socks5" { "socks5h" } else { &p.protocol }),
        "proxyAddress": proxy.map(|p| text(&p.address)),
        "proxyPort": proxy.map(|p| p.port),
    }).to_string()
}

pub(super) fn completed(
    platform: &str,
    request_id: &str,
    video: &VideoMetadata,
    elapsed: Duration,
) -> Vec<LogEntry> {
    let labels = unique(
        video
            .formats
            .iter()
            .filter_map(|f| f.quality_label.as_deref())
            .filter(|label| !label.trim().is_empty())
            .map(text),
    );
    let resolutions = unique(
        video
            .formats
            .iter()
            .filter_map(|f| match (f.width, f.height) {
                (Some(w), Some(h)) => Some(format!("{w}x{h}")),
                (None, Some(h)) => Some(format!("?x{h}")),
                (Some(w), None) => Some(format!("{w}x?")),
                _ => None,
            }),
    );
    let mut entries = vec![entry(
        "info",
        platform,
        Some(&video.id),
        Some(request_id),
        "parseCompleted",
        None,
        serde_json::json!({
            "elapsedMs": elapsed.as_millis().min(u64::MAX as u128) as u64,
            "formatCount": video.formats.len(),
            "defaultFormatId": video.default_format_id,
            "durationSeconds": video.duration,
            "dimensionLabels": video.formats.iter().filter(|f| f.quality_label_source.as_deref() == Some("dimensions")).count(),
            "unknownCodec": video.formats.iter().filter(|f| f.codec_label.is_none()).count(),
            "unknownFps": video.formats.iter().filter(|f| f.fps.is_none()).count(),
            "unknownBitrate": video.formats.iter().filter(|f| f.bitrate.is_none()).count(),
            "unknownSize": video.formats.iter().filter(|f| f.size_bytes.is_none()).count(),
            "estimatedSize": video.formats.iter().filter(|f| f.size_approximate).count(),
            // Avoid the word Cookie: the shared logger redacts it in messages.
            "publicFallback": video.cookie_fallback,
            "qualityLabels": labels.iter().take(12).collect::<Vec<_>>(),
            "qualityLabelsOmitted": labels.len().saturating_sub(12),
            "resolutions": resolutions.iter().take(12).collect::<Vec<_>>(),
            "resolutionsOmitted": resolutions.len().saturating_sub(12),
            "unlabeledFormats": video.formats.iter().filter(|f|
                f.quality_label.as_deref().is_none_or(|s| s.trim().is_empty())).count(),
        })
            .to_string(),
    )];
    for f in &video.formats {
        entries.push(entry(
            "debug",
            platform,
            Some(&video.id),
            Some(request_id),
            "parseFormat",
            None,
            serde_json::json!({
                "formatId": text(&f.format_id),
                // This is the extracted backend label, not a reconstructed UI label.
                "qualityLabel": f.quality_label.as_deref().map(text),
                "qualityLabelSource": f.quality_label_source,
                "formatNote": f.raw_format_note,
                "formatDescription": f.raw_format_description,
                "codecLabel": f.codec_label.as_deref().map(text),
                "width": f.width, "height": f.height, "fps": f.fps,
                "videoCodec": f.video_codec.as_deref().map(text),
                "extension": f.extension.as_deref().map(text),
                "bitrateBps": f.bitrate, "sizeBytes": f.size_bytes,
                "sizeApproximate": f.size_approximate, "watermarked": f.watermarked,
            })
                .to_string(),
        ));
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    #[test]
    fn network_logs_identify_explicit_proxy_and_engine_without_private_command_data() {
        let proxy = crate::proxy::ProxySettings {
            protocol: "socks5".into(),
            address: "::1".into(),
            port: 7890,
        };
        for (platform, engine, argument) in [
            ("youtube", "yt-dlp", Some("--proxy")),
            ("bilibili", "yt-dlp", Some("--proxy")),
            ("douyin", "native-http", None),
        ] {
            let message = network_summary(platform, Some(&proxy));
            let detail: Value = serde_json::from_str(&message).unwrap();
            assert_eq!(detail["engine"], engine);
            assert_eq!(detail["proxyEnabled"], true);
            assert_eq!(detail["proxyMode"], "explicit");
            assert_eq!(detail["proxyProtocol"], "socks5h");
            assert_eq!(detail["proxyAddress"], "::1");
            assert_eq!(detail["proxyPort"], 7890);
            assert_eq!(detail["proxyArgument"], json!(argument));
        }
        for (platform, mode) in [("youtube", "system"), ("bilibili", "system"), ("douyin", "direct")] {
            let detail: Value = serde_json::from_str(&network_summary(platform, None)).unwrap();
            assert_eq!(detail["proxyEnabled"], false);
            assert_eq!(detail["proxyMode"], mode);
            for field in ["proxyProtocol", "proxyAddress", "proxyPort", "proxyArgument"] {
                assert!(detail[field].is_null());
            }
        }
    }

    #[test]
    fn network_configuration_is_visible_in_release_logs_with_request_context() {
        let root = tempfile::tempdir().unwrap();
        eprintln!("owned network log test directory: {}", root.path().display());
        let (logger, handle) = crate::app_logs::builder(root.path(), false)
            .unwrap().build().unwrap();
        let store = crate::app_logs::AppLogStore::new(logger.into(), handle);
        let proxy = crate::proxy::ProxySettings {
            protocol: "http".into(),
            address: "127.0.0.1".into(),
            port: 7890,
        };
        for event in ["parseNetworkConfigured", "downloadNetworkConfigured"] {
            store.append(entry("info", "youtube", Some("video-1"), Some("request-1"),
                               event, None, network_summary("youtube", Some(&proxy)))).unwrap();
        }
        let disk = std::fs::read_to_string(root.path().join("logs/application_rCURRENT.log")).unwrap();
        let entries: Vec<Value> = disk.lines()
            .map(|line| serde_json::from_str(&line[line.find('{').unwrap()..]).unwrap()).collect();
        assert_eq!(entries.len(), 2);
        for (item, event) in entries.iter().zip(["parseNetworkConfigured", "downloadNetworkConfigured"]) {
            assert_eq!(item["event"], event);
            assert_eq!(item["requestId"], "request-1");
            assert_eq!(item["platform"], "youtube");
            assert_eq!(item["videoId"], "video-1");
            let detail: Value = serde_json::from_str(item["message"].as_str().unwrap()).unwrap();
            assert_eq!(detail["proxyEnabled"], true);
            assert_eq!(detail["proxyAddress"], "127.0.0.1");
            assert_eq!(detail["proxyPort"], 7890);
            assert_eq!(detail["proxyArgument"], "--proxy");
        }
        for private_field in ["Cookie", "Authorization", "password", "token", "https://"] {
            assert!(!disk.contains(private_field));
        }
    }

    #[test]
    fn summary_bounds_large_format_lists_and_keeps_missing_data_unknown() {
        let mut video = super::super::parse_metadata(
            br#"{"id":"v","title":"t","formats":[{"format_id":"v","height":1080}]}"#,
        )
            .unwrap();
        video.formats = (0..50)
            .map(|i| super::super::VideoFormat {
                format_id: i.to_string(),
                quality_label: Some(format!("{i}{}", "\"".repeat(300))),
                ..Default::default()
            })
            .collect();
        video.formats.push(video.formats[0].clone());
        let entries = completed("youtube", "r", &video, Duration::ZERO);
        assert!(entries[0].message.chars().count() < 4096);
        let summary: Value = serde_json::from_str(&entries[0].message).unwrap();
        assert_eq!(summary["formatCount"], 51);
        assert_eq!(summary["qualityLabels"].as_array().unwrap().len(), 12);
        assert_eq!(summary["qualityLabelsOmitted"], 38);
        assert_eq!(summary["resolutions"], json!([]));
        let detail: Value = serde_json::from_str(&entries[1].message).unwrap();
        assert!(detail["width"].is_null());
        assert!(detail["fps"].is_null());
        video.formats[0].quality_label = Some("https://media.test?token=private".into());
        let entries = completed("youtube", "r", &video, Duration::ZERO);
        assert!(!entries.iter().any(|e| e.message.contains("token=private")));
    }

    #[test]
    fn parse_diagnostics_preserve_results_and_exclude_private_metadata_on_disk() {
        let mut video = super::super::parse_metadata(&serde_json::to_vec(&json!({
            "id": "video-123", "title": "private-title", "thumbnail": "https://private-cover.test",
            "extractor_key": "BiliBili", "formats": [
                {"format_id": "120", "format": "4K 超高清", "width":3840,"height":2160,
                 "fps":60,"vcodec":"hev1","tbr":8000,"filesize_approx":12345,
                 "url":"https://private-media.test?token=secret", "http_headers":{"Cookie":"private-session"}},
                {"format_id":"portrait","width":1080,"height":1920,"vcodec":"avc1"}
            ]
        })).unwrap()).unwrap();
        video.cookie_fallback = true;
        video.formats[0].native_result_id = Some("private-cache-key".into());
        for development in [false, true] {
            let root = tempfile::tempdir().unwrap();
            eprintln!(
                "owned parse diagnostics test directory: {}",
                root.path().display()
            );
            let (logger, handle) = crate::app_logs::builder(root.path(), development)
                .unwrap()
                .build()
                .unwrap();
            let store = crate::app_logs::AppLogStore::new(logger.into(), handle);
            for item in completed(
                "bilibili",
                "request-123",
                &video,
                Duration::from_millis(1500),
            ) {
                store.append(item).unwrap();
            }
            let disk =
                std::fs::read_to_string(root.path().join("logs/application_rCURRENT.log")).unwrap();
            for secret in [
                "private-title",
                "private-cover",
                "private-media",
                "private-session",
                "private-cache-key",
            ] {
                assert!(!disk.contains(secret), "leaked {secret}");
            }
            let entries: Vec<Value> = disk
                .lines()
                .map(|line| serde_json::from_str(&line[line.find('{').unwrap()..]).unwrap())
                .collect();
            assert_eq!(entries.len(), if development { 3 } else { 1 });
            assert!(entries
                .iter()
                .all(|e| e["requestId"] == "request-123" && e["videoId"] == "video-123"));
            let summary: Value =
                serde_json::from_str(entries[0]["message"].as_str().unwrap()).unwrap();
            assert_eq!(summary["elapsedMs"], 1500);
            assert_eq!(summary["formatCount"], 2);
            assert_eq!(summary["publicFallback"], true);
            assert_eq!(summary["qualityLabels"], json!(["4K 超高清", "1080P"]));
            assert_eq!(summary["resolutions"], json!(["3840x2160", "1080x1920"]));
            assert_eq!(summary["unlabeledFormats"], 0);
            if development {
                let detail: Value =
                    serde_json::from_str(entries[1]["message"].as_str().unwrap()).unwrap();
                assert_eq!(detail["qualityLabel"], "4K 超高清");
                assert_eq!(detail["bitrateBps"], 8_000_000);
                assert_eq!(detail["sizeBytes"], 12345);
                assert_eq!(detail["sizeApproximate"], true);
                let missing: Value =
                    serde_json::from_str(entries[2]["message"].as_str().unwrap()).unwrap();
                assert_eq!(missing["qualityLabel"], "1080P");
                assert_eq!(missing["qualityLabelSource"], "dimensions");
                assert_eq!(detail["qualityLabelSource"], "format");
                assert_eq!(detail["formatDescription"], "4K 超高清");
                assert_eq!(missing["width"], 1080);
                assert_eq!(missing["height"], 1920);
            }
        }
    }
}
