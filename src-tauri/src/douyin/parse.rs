use super::{Candidate, LabError, LabFormat, LabVideo, ParsedResult};
use serde_json::Value;
use sha2::{Digest, Sha256};
use url::Url;

pub(super) fn douyin_host(host: &str) -> bool {
    host == "douyin.com"
        || host.ends_with(".douyin.com")
        || host == "iesdouyin.com"
        || host.ends_with(".iesdouyin.com")
}
pub(crate) fn normalize_link(input: &str) -> Result<Url, LabError> {
    let start = input
        .find("https://")
        .ok_or_else(|| LabError::new("invalidLink", "Expected a Douyin HTTPS link"))?;
    let text = input[start..]
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_end_matches(['，', '。', '）', ')', '！', '!']);
    let url = Url::parse(text).map_err(|_| LabError::new("invalidLink", "Invalid video link"))?;
    if url.scheme() != "https"
        || !url.host_str().is_some_and(douyin_host)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(LabError::new("invalidLink", "Expected a Douyin HTTPS link"));
    }
    Ok(url)
}
pub(crate) fn parse_detail(value: &Value) -> Result<ParsedResult, LabError> {
    let detail = value
        .get("aweme_detail")
        .ok_or_else(|| LabError::new("emptyResponse", "No video detail returned"))?;
    let id = detail["aweme_id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| LabError::new("emptyResponse", "Missing video identity"))?;
    if detail["images"].as_array().is_some_and(|a| !a.is_empty()) {
        return Err(LabError::new(
            "unsupported",
            "Image posts are not supported",
        ));
    }
    let video = &detail["video"];
    let mut candidates: Vec<Candidate> = Vec::new();
    let number = |v: &Value| v.as_u64().filter(|n| *n > 0);
    let mut add = |addr: &Value, rate: &Value, codec: &str, watermarked: Option<bool>| {
        let urls: Vec<String> = addr["url_list"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|s| {
                Url::parse(s).is_ok_and(|u| {
                    matches!(u.scheme(), "https" | "http")
                        && u.host_str().is_some()
                        && u.username().is_empty()
                        && u.password().is_none()
                })
            })
            .map(str::to_owned)
            .collect();
        if urls.is_empty() {
            return;
        }
        let width = number(&addr["width"]).and_then(|n| u32::try_from(n).ok());
        let height = number(&addr["height"]).and_then(|n| u32::try_from(n).ok());
        let fps = rate["FPS"]
            .as_f64()
            .or_else(|| rate["fps"].as_f64())
            .filter(|n| n.is_finite() && *n > 0.0);
        let bitrate = number(&rate["bit_rate"]);
        let size = number(&addr["data_size"]);
        let key = format!(
            "{id}|{codec}|{width:?}|{height:?}|{fps:?}|{bitrate:?}|{size:?}|{watermarked:?}|{}",
            addr["url_key"].as_str().unwrap_or("")
        );
        let format_id = format!("{:x}", Sha256::digest(key.as_bytes()));
        if let Some(found) = candidates.iter_mut().find(|c| c.format.id == format_id) {
            for url in urls {
                if !found.urls.contains(&url) {
                    found.urls.push(url);
                }
            }
        } else {
            candidates.push(Candidate {
                format: LabFormat {
                    id: format_id,
                    width,
                    height,
                    codec: codec.into(),
                    fps,
                    bitrate,
                    file_size: size,
                    watermarked,
                },
                urls,
            });
        }
    };
    if let Some(rates) = video["bit_rate"].as_array() {
        for rate in rates {
            let codec = if rate["is_bytevc1"].as_bool() == Some(true)
                || rate["is_bytevc1"].as_u64().unwrap_or(0) == 1
                || rate["gear_name"].as_str().unwrap_or("").contains("bytevc1")
                || rate["play_addr"]["url_key"]
                .as_str()
                .unwrap_or("")
                .contains("bytevc1")
            {
                "hevc"
            } else {
                "h264"
            };
            add(&rate["play_addr"], rate, codec, Some(false));
        }
    }
    for (field, codec, watermark) in [
        ("play_addr", "unknown", Some(false)),
        ("play_addr_h264", "h264", Some(false)),
        ("play_addr_bytevc1", "hevc", Some(false)),
        (
            "download_addr",
            "unknown",
            video["has_watermark"].as_bool().or(Some(true)),
        ),
    ] {
        let addr = &video[field];
        let key = addr["url_key"].as_str().unwrap_or("");
        let codec = if key.contains("bytevc1") {
            "hevc"
        } else if key.contains("h264") {
            "h264"
        } else {
            codec
        };
        add(addr, &Value::Null, codec, watermark);
    }
    if candidates.is_empty() {
        return Err(LabError::new(
            "noFormats",
            "No downloadable video formats returned",
        ));
    }
    // Extraction order is not a preference policy. The shared VideoFormat
    // adapter owns business ordering and default selection for every platform.
    let public = LabVideo {
        result_id: uuid::Uuid::new_v4().to_string(),
        video_id: id.into(),
        title: detail["desc"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(id)
            .into(),
        duration: video["duration"]
            .as_f64()
            .filter(|n| *n > 0.0)
            .map(|n| n / 1000.0),
        cover: video["cover"]["url_list"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .find(|s| s.starts_with("https://"))
            .map(str::to_owned),
        formats: candidates.iter().map(|c| c.format.clone()).collect(),
    };
    Ok(ParsedResult {
        video: public,
        candidates,
    })
}
