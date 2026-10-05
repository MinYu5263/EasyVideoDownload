#[cfg(test)]
mod tests;

mod cancel;
mod cookies;
mod download;
mod http;
pub(crate) mod lab;
mod parse;
pub(crate) use parse::{normalize_link, parse_detail};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct LabError {
    pub code: &'static str,
    pub detail: String,
}
impl LabError {
    pub fn new(code: &'static str, detail: &str) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
    pub fn cancelled() -> Self {
        Self::new("cancelled", "Operation cancelled")
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LabFormat {
    pub id: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub codec: String,
    pub fps: Option<f64>,
    pub bitrate: Option<u64>,
    pub file_size: Option<u64>,
    pub watermarked: Option<bool>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LabVideo {
    pub result_id: String,
    pub video_id: String,
    pub title: String,
    pub duration: Option<f64>,
    pub cover: Option<String>,
    pub formats: Vec<LabFormat>,
}
#[derive(Clone)]
pub(crate) struct Candidate {
    pub format: LabFormat,
    pub urls: Vec<String>,
}
#[derive(Clone)]
pub(crate) struct ParsedResult {
    pub video: LabVideo,
    pub candidates: Vec<Candidate>,
}
