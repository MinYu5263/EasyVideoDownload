//! Shared business representation for parsers, persisted pages and history.
use super::VideoFormat;
use std::cmp::Ordering;

pub(crate) const MAX_EXACT_INTEGER: u64 = 9_007_199_254_740_991;

pub(crate) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "_.-:".contains(c))
}

pub(crate) fn meaningful(text: &str) -> Option<&str> {
    let text = text.trim();
    (!text.is_empty()
        && !matches!(
            text.to_ascii_lowercase().as_str(),
            "unknown" | "none" | "n/a" | "-"
        ))
        .then_some(text)
}

pub(crate) fn codec_label(codec: &str) -> Option<String> {
    let codec = meaningful(codec)?;
    let lower = codec.to_ascii_lowercase();
    let label = if ["avc", "h264", "h.264"]
        .iter()
        .any(|p| lower.starts_with(p))
    {
        "H.264"
    } else if ["hev", "hvc", "h265", "h.265", "bytevc1"]
        .iter()
        .any(|p| lower.starts_with(p))
    {
        "H.265"
    } else if lower.starts_with("av01") || lower.starts_with("av1") {
        "AV1"
    } else if lower.starts_with("vp9") || lower.starts_with("vp09") {
        "VP9"
    } else if lower.starts_with("vp8") || lower.starts_with("vp08") {
        "VP8"
    } else {
        codec
    };
    Some(label.to_owned())
}

fn resolution(f: &VideoFormat) -> (u32, u32) {
    match (f.width, f.height) {
        (Some(w), Some(h)) => (w.min(h), w.max(h)),
        (_, Some(h)) => (h, h),
        _ => (0, 0),
    }
}

pub(crate) fn normalize(f: &mut VideoFormat) {
    f.width = f.width.filter(|n| *n > 0);
    f.height = f.height.filter(|n| *n > 0);
    f.fps = f.fps.filter(|n| n.is_finite() && *n > 0.0);
    f.bitrate = f.bitrate.filter(|n| *n > 0 && *n <= MAX_EXACT_INTEGER);
    f.size_bytes = f.size_bytes.filter(|n| *n > 0 && *n <= MAX_EXACT_INTEGER);
    f.size_approximate &= f.size_bytes.is_some();
    f.extension = f
        .extension
        .as_deref()
        .and_then(meaningful)
        .map(str::to_ascii_lowercase);
    f.video_codec = f
        .video_codec
        .as_deref()
        .and_then(meaningful)
        .map(str::to_owned);
    f.codec_label = f.video_codec.as_deref().and_then(codec_label);
    let label = f
        .quality_label
        .as_deref()
        .and_then(meaningful)
        .map(str::to_uppercase);
    if let Some(label) = label {
        f.quality_label = Some(label);
        if !matches!(
            f.quality_label_source.as_deref(),
            Some("format_note" | "format" | "dimensions" | "persisted")
        ) {
            f.quality_label_source = Some("persisted".into());
        }
    } else {
        let short = resolution(f).0;
        f.quality_label = (short > 0).then(|| format!("{short}P"));
        f.quality_label_source = (short > 0).then(|| "dimensions".into());
    }
}

fn codec_rank(f: &VideoFormat) -> u8 {
    match f.codec_label.as_deref() {
        Some("H.264") => 0,
        Some("H.265") => 1,
        Some("AV1") => 2,
        Some(_) => 3,
        None => 4,
    }
}

fn preference(a: &VideoFormat, b: &VideoFormat) -> Ordering {
    resolution(b)
        .cmp(&resolution(a))
        .then_with(|| codec_rank(a).cmp(&codec_rank(b)))
        .then_with(|| {
            if codec_rank(a) == 3 {
                a.codec_label.cmp(&b.codec_label)
            } else {
                Ordering::Equal
            }
        })
        .then_with(|| b.fps.unwrap_or(0.0).total_cmp(&a.fps.unwrap_or(0.0)))
        .then_with(|| b.bitrate.cmp(&a.bitrate))
        .then_with(|| a.format_id.cmp(&b.format_id))
}

/// Deterministic business order. UI-driven column sorting can still be local.
pub(crate) fn normalize_all(formats: &mut [VideoFormat]) {
    formats.iter_mut().for_each(normalize);
    formats.sort_by(preference);
}

pub(crate) fn default_id(formats: &[VideoFormat]) -> Option<String> {
    formats
        .iter()
        .find(|f| f.watermarked != Some(true))
        .or_else(|| formats.first())
        .map(|f| f.format_id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_skips_explicit_watermarks_and_normalization_is_idempotent() {
        let mut formats = vec![
            VideoFormat {
                format_id: "watermark".into(),
                height: Some(2160),
                watermarked: Some(true),
                ..Default::default()
            },
            VideoFormat {
                format_id: "plain".into(),
                width: Some(1080),
                height: Some(1920),
                video_codec: Some("BYTEVC1".into()),
                ..Default::default()
            },
        ];
        normalize_all(&mut formats);
        assert_eq!(default_id(&formats).as_deref(), Some("plain"));
        assert_eq!(formats[1].quality_label.as_deref(), Some("1080P"));
        assert_eq!(formats[1].codec_label.as_deref(), Some("H.265"));
        let once = formats.clone();
        normalize_all(&mut formats);
        assert_eq!(formats, once);
        formats[1].watermarked = Some(true);
        assert_eq!(default_id(&formats).as_deref(), Some("watermark"));
    }

    #[test]
    fn invalid_optional_numbers_remain_unknown() {
        let mut f = VideoFormat {
            width: Some(0),
            height: Some(0),
            fps: Some(f64::INFINITY),
            bitrate: Some(u64::MAX),
            size_bytes: Some(u64::MAX),
            size_approximate: true,
            ..Default::default()
        };
        normalize(&mut f);
        assert_eq!(
            (f.width, f.height, f.fps, f.bitrate, f.size_bytes),
            (None, None, None, None, None)
        );
        assert!(!f.size_approximate);
        assert_eq!(f.quality_label, None);
    }

    #[test]
    fn preference_uses_short_side_codec_fps_bitrate_and_stable_id() {
        let make = |id: &str, w, h, codec: &str, fps, bitrate| VideoFormat {
            format_id: id.into(),
            width: Some(w),
            height: Some(h),
            video_codec: Some(codec.into()),
            fps: Some(fps),
            bitrate: Some(bitrate),
            ..Default::default()
        };
        let mut formats = vec![
            make("720", 720, 1280, "avc1", 60.0, 9000),
            make("av1", 1920, 1080, "av01", 120.0, 9000),
            make("hevc", 1920, 1080, "hev1", 120.0, 8000),
            make("h264-30", 1920, 1080, "avc1", 30.0, 8000),
            make("h264-low", 1080, 1920, "avc1", 60.0, 2000),
            make("h264-high", 1920, 1080, "avc1", 60.0, 5000),
        ];
        normalize_all(&mut formats);
        assert_eq!(
            formats
                .iter()
                .map(|f| f.format_id.as_str())
                .collect::<Vec<_>>(),
            ["h264-high", "h264-low", "h264-30", "hevc", "av1", "720"]
        );
    }
}
