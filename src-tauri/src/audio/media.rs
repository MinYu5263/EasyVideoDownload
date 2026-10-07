use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioTrack {
    pub index: u32,
    pub codec: String,
    pub codec_label: String,
    pub output_format: String,
    pub duration: Option<f64>,
    pub bitrate: Option<u64>,
    pub sample_rate: Option<u64>,
    pub channels: Option<u64>,
    pub language: Option<String>,
    pub title: Option<String>,
    pub is_default: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioInfo {
    pub id: String,
    pub file_name: String,
    pub tracks: Vec<AudioTrack>,
    pub default_track: u32,
}

fn positive_number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|n| n.is_finite() && *n > 0.0)
}

fn positive_integer(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|n| *n > 0)
}

fn stream_duration(stream: &Value) -> Option<f64> {
    positive_number(&stream["duration"]).or_else(|| {
        let mut fields = stream["tags"]["DURATION"].as_str()?.split(':');
        let hours: f64 = fields.next()?.parse().ok()?;
        let minutes: f64 = fields.next()?.parse().ok()?;
        let seconds: f64 = fields.next()?.parse().ok()?;
        if fields.next().is_some()
            || hours < 0.0
            || !(0.0..60.0).contains(&minutes)
            || !(0.0..60.0).contains(&seconds)
        {
            return None;
        }
        let duration = hours * 3600.0 + minutes * 60.0 + seconds;
        (duration.is_finite() && duration > 0.0).then_some(duration)
    })
}

fn tag(stream: &Value, name: &str) -> Option<String> {
    stream["tags"][name]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "und")
        .map(crate::app_logs::safe_text)
}

pub(super) fn parse_tracks(value: &Value) -> Result<Vec<AudioTrack>, super::AudioError> {
    let streams = value["streams"].as_array().ok_or_else(|| {
        super::AudioError::new("invalidMedia", "FFprobe returned no media streams")
    })?;
    let tracks: Vec<_> = streams
        .iter()
        .filter(|s| s["codec_type"] == "audio")
        .filter_map(|s| {
            let index = u32::try_from(s["index"].as_u64()?).ok()?;
            let codec = s["codec_name"]
                .as_str()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            let codec_label = match codec.as_str() {
                "opus" => "Opus".into(),
                "vorbis" => "Vorbis".into(),
                "eac3" => "E-AC-3".into(),
                "ac3" => "AC-3".into(),
                c if c.starts_with("pcm_") => "PCM".into(),
                c => c.to_ascii_uppercase(),
            };
            Some(AudioTrack {
                index,
                output_format: output_format(&codec).0.to_ascii_uppercase(),
                codec,
                codec_label,
                duration: stream_duration(s),
                bitrate: positive_integer(&s["bit_rate"]),
                sample_rate: positive_integer(&s["sample_rate"]),
                channels: positive_integer(&s["channels"]),
                language: tag(s, "language"),
                title: tag(s, "title"),
                is_default: s["disposition"]["default"].as_u64() == Some(1),
            })
        })
        .collect();
    if tracks.is_empty() {
        return Err(super::AudioError::new(
            "noAudio",
            "No audio stream was found in this video",
        ));
    }
    Ok(tracks)
}

pub(super) fn output_format(codec: &str) -> (&'static str, &'static str) {
    match codec {
        "aac" | "alac" => ("m4a", "ipod"),
        "mp3" => ("mp3", "mp3"),
        "opus" => ("opus", "opus"),
        "vorbis" => ("ogg", "ogg"),
        "flac" => ("flac", "flac"),
        "ac3" => ("ac3", "ac3"),
        "eac3" => ("eac3", "eac3"),
        "dts" => ("dts", "dts"),
        c if c.starts_with("pcm_") => ("wav", "wav"),
        _ => ("mka", "matroska"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn audio_tracks_keep_real_indices_defaults_and_unknowns() {
        let tracks = parse_tracks(&json!({"format":{"duration":"12.5"},"streams":[
            {"index":0,"codec_type":"video","codec_name":"h264"},
            {"index":2,"codec_type":"audio","codec_name":"aac","sample_rate":"48000","channels":2,"bit_rate":"128000","tags":{"language":"eng"}},
            {"index":4,"codec_type":"audio","codec_name":"opus","duration":"NaN","sample_rate":"0","channels":0,"bit_rate":"-1","disposition":{"default":1}}
        ]})).unwrap();
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].index, 2);
        assert_eq!(tracks[0].codec_label, "AAC");
        assert_eq!(tracks[0].language.as_deref(), Some("eng"));
        assert_eq!(tracks[1].duration, None);
        assert_eq!(tracks[1].sample_rate, None);
        assert_eq!(tracks[1].bitrate, None);
        assert_eq!(tracks[1].channels, None);
        assert!(tracks[1].is_default);
    }

    #[test]
    fn video_without_audio_has_a_specific_failure() {
        let error =
            parse_tracks(&json!({"streams":[{"index":0,"codec_type":"video"}]})).unwrap_err();
        assert_eq!(error.code, "noAudio");
    }

    #[test]
    fn audio_duration_uses_stream_duration_instead_of_video_duration() {
        let tracks = parse_tracks(&json!({"format":{"duration":"120"},"streams":[
            {"index":1,"codec_type":"audio","codec_name":"aac","tags":{"DURATION":"00:00:01.024000000"}}
        ]})).unwrap();
        assert_eq!(tracks[0].duration, Some(1.024));
    }

    #[test]
    fn muxers_preserve_audio_encoding_without_transcoding() {
        assert_eq!(output_format("aac"), ("m4a", "ipod"));
        assert_eq!(output_format("opus"), ("opus", "opus"));
        assert_eq!(output_format("pcm_s16le"), ("wav", "wav"));
        assert_eq!(output_format("unrecognized_codec"), ("mka", "matroska"));
    }

    #[test]
    fn serialized_tracks_expose_the_actual_extraction_format() {
        for (codec, expected) in [
            ("aac", "M4A"),
            ("alac", "M4A"),
            ("mp3", "MP3"),
            ("opus", "OPUS"),
            ("vorbis", "OGG"),
            ("flac", "FLAC"),
            ("ac3", "AC3"),
            ("eac3", "EAC3"),
            ("dts", "DTS"),
            ("pcm_s16le", "WAV"),
            ("unrecognized_codec", "MKA"),
        ] {
            let tracks = parse_tracks(&json!({"streams":[
                {"index":1,"codec_type":"audio","codec_name":codec}
            ]}))
                .unwrap();
            let serialized = serde_json::to_value(&tracks[0]).unwrap();
            assert_eq!(serialized["outputFormat"], expected, "codec={codec}");
        }
    }
}
