use super::DownloadProgress;
use serde_json::Value;
use std::collections::BTreeMap;

// Reserve the last part of the task for real postprocessing and file confirmation.
// FFmpeg invoked by yt-dlp reports stage boundaries, not continuous merge progress.
const DOWNLOAD_SHARE: f64 = 95.0;
#[derive(Default)]
struct StreamProgress {
    total: Option<f64>,
    downloaded: f64,
    finished: bool,
}
#[derive(Default)]
pub(super) struct ProgressTracker {
    streams: BTreeMap<String, StreamProgress>,
    planned: Vec<String>,
    combined_id: String,
    percentage: f64,
    last_speed: Option<f64>,
    started: bool,
    processing: bool,
}
fn nonnegative(value: &Value) -> Option<f64> {
    value.as_f64().filter(|n| n.is_finite() && *n >= 0.0)
}
fn positive(value: &Value) -> Option<f64> {
    nonnegative(value).filter(|n| *n > 0.0)
}

impl ProgressTracker {
    fn task_progress(&mut self, speed: Option<f64>, eta: Option<f64>) -> DownloadProgress {
        // Some downloaders fetch several formats together (for example through FFmpeg).
        // Use that combined stream's real byte count instead of counting it twice.
        let combined = !self.planned.contains(&self.combined_id)
            && self.streams.contains_key(&self.combined_id);
        let ids = if combined {
            std::slice::from_ref(&self.combined_id)
        } else {
            self.planned.as_slice()
        };
        if !ids.is_empty() {
            let streams: Vec<_> = ids.iter().filter_map(|id| self.streams.get(id)).collect();
            let known_sizes = streams.iter().all(|stream| stream.total.is_some());
            let total_weight: f64 = streams
                .iter()
                .map(|stream| {
                    if known_sizes {
                        stream.total.unwrap()
                    } else {
                        1.0
                    }
                })
                .sum();
            if total_weight > 0.0 {
                let completed_weight: f64 = streams
                    .iter()
                    .map(|stream| {
                        let fraction = if stream.finished {
                            1.0
                        } else {
                            stream
                                .total
                                .map(|total| (stream.downloaded / total).clamp(0.0, 1.0))
                                .unwrap_or(0.0)
                        };
                        fraction
                            * if known_sizes {
                                stream.total.unwrap()
                            } else {
                                1.0
                            }
                    })
                    .sum();
                let measured = DOWNLOAD_SHARE * completed_weight / total_weight;
                // Size estimates and retries can change the denominator, but the task
                // must not visibly go backwards when a later audio stream starts.
                self.percentage = self.percentage.max(measured);
            }
            if streams.iter().all(|stream| stream.finished) {
                self.processing = true;
            }
        }
        // Missing samples do not invalidate the last measured transfer speed.
        // Postprocessing no longer represents an active network transfer.
        self.last_speed = if self.processing {
            None
        } else {
            speed.or(self.last_speed)
        };
        DownloadProgress {
            phase: if self.processing {
                "processing"
            } else if self.started {
                "downloading"
            } else {
                "preparing"
            },
            percent: Some(self.percentage),
            speed: self.last_speed,
            eta: if self.processing { None } else { eta },
        }
    }

    pub(super) fn update(&mut self, line: &str) -> Option<DownloadProgress> {
        if let Some(json) = line.strip_prefix("__EVD_PLAN__") {
            let value: Value = serde_json::from_str(json).ok()?;
            self.combined_id = value["formatId"].as_str()?.to_owned();
            let formats = value["formats"].as_array()?;
            if formats.is_empty() {
                if !self.planned.contains(&self.combined_id) {
                    self.planned.push(self.combined_id.clone());
                }
                let stream = self.streams.entry(self.combined_id.clone()).or_default();
                stream.total = stream.total.or_else(|| positive(&value["size"]));
            } else {
                for format in formats {
                    let Some(id) = format["format_id"].as_str().filter(|id| !id.is_empty()) else {
                        continue;
                    };
                    if !self.planned.iter().any(|planned| planned == id) {
                        self.planned.push(id.to_owned());
                    }
                    let stream = self.streams.entry(id.to_owned()).or_default();
                    stream.total = stream.total.or_else(|| {
                        positive(&format["filesize"])
                            .or_else(|| positive(&format["filesize_approx"]))
                    });
                }
            }
            return Some(self.task_progress(None, None));
        }
        if let Some(json) = line.strip_prefix("__EVD_PROCESSING__") {
            let value: Value = serde_json::from_str(json).ok()?;
            if !matches!(value["status"].as_str(), Some("started" | "finished")) {
                return None;
            }
            // Pre-download hooks must not masquerade as a completed download phase.
            if self.planned.is_empty() && !self.started {
                return None;
            }
            self.processing = true;
            self.percentage = self.percentage.max(DOWNLOAD_SHARE);
            return Some(self.task_progress(None, None));
        }
        let json = line.strip_prefix("__EVD_PROGRESS__")?;
        let value: Value = serde_json::from_str(json).ok()?;
        let id = value["formatId"].as_str()?;
        let progress = &value["progress"];
        let status = progress["status"].as_str()?;
        if !matches!(status, "downloading" | "finished") {
            return None;
        }
        self.started = true;
        let stream = self.streams.entry(id.to_owned()).or_default();
        if let Some(total) = positive(&progress["total_bytes"])
            .or_else(|| positive(&progress["total_bytes_estimate"]))
        {
            stream.total = Some(total);
        }
        if let Some(bytes) = nonnegative(&progress["downloaded_bytes"]) {
            stream.downloaded = bytes;
        }
        if status == "finished" {
            stream.finished = true;
        }
        Some(self.task_progress(
            if status == "downloading" {
                nonnegative(&progress["speed"])
            } else {
                None
            },
            if status == "downloading" {
                nonnegative(&progress["eta"])
            } else {
                None
            },
        ))
    }
    pub(super) fn finalizing(&mut self) -> DownloadProgress {
        self.processing = true;
        self.percentage = 99.0;
        self.task_progress(None, None)
    }
}
