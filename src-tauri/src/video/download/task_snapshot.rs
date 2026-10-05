use super::*;
use crate::{database::page_states::DownloadPageState, required_tools::usage::ToolUsageLease};

pub(super) struct TaskExecutionSnapshot {
    pub page: DownloadPageState,
    pub settings: RequiredToolSettings,
    pub platform: CookiePlatform,
    pub runner: DownloadExecution,
    pub network_diagnostic: String,
    pub command_options: Option<(
        DownloadCommandOptions,
        Option<crate::proxy::ProxySettings>,
        String,
    )>,
    pub _cookie: Option<tempfile::NamedTempFile>,
    pub _tools: ToolUsageLease,
}
pub(super) enum DownloadExecution {
    Ytdlp(Command),
    Douyin(crate::video::native_douyin::NativeExecution),
}
impl TaskExecutionSnapshot {
    pub(super) fn set_temporary_directory(&mut self, directory: &Path) -> Result<(), VideoError> {
        if let Some((options, proxy, url)) = &self.command_options {
            self.runner = DownloadExecution::Ytdlp(
                super::super::commands::download_command_with_temporary_directory(
                    &self.settings,
                    url,
                    self._cookie.as_ref().map(|f| f.path()),
                    options,
                    proxy.as_ref(),
                    Some(directory),
                )?,
            );
        }
        Ok(())
    }
    pub(super) async fn check_tools(&self) -> Result<(), VideoError> {
        match &self.runner {
            DownloadExecution::Ytdlp(_) => {
                check_download_tool_files(&self.settings, self.platform).await
            }
            DownloadExecution::Douyin(native) => native.check_tools(),
        }
    }
}
#[cfg(test)]
impl TaskExecutionSnapshot {
    pub(super) fn ytdlp_command(&self) -> &Command {
        match &self.runner {
            DownloadExecution::Ytdlp(c) => c,
            _ => panic!("Expected yt-dlp task"),
        }
    }
}

pub(super) async fn capture_task_snapshot(
    page: DownloadPageState,
    tools: &RequiredToolManager,
    cookies: &CookieStore,
    storage: &Storage,
) -> Result<TaskExecutionSnapshot, VideoError> {
    capture_snapshot(page, tools, cookies, storage, false).await
}

pub(super) async fn capture_record_snapshot(
    record: &crate::database::download_records::DownloadRecord,
    tools: &RequiredToolManager,
    cookies: &CookieStore,
    storage: &Storage,
) -> Result<TaskExecutionSnapshot, VideoError> {
    capture_snapshot(page_from_record(record)?, tools, cookies, storage, true).await
}

fn page_from_record(
    record: &crate::database::download_records::DownloadRecord,
) -> Result<DownloadPageState, VideoError> {
    let previous = record
        .successful_output
        .as_ref()
        .filter(|_| record.status != "paused");
    let saved = previous
        .map(|p| p.format_snapshot.clone())
        .unwrap_or_else(|| record.format_snapshot.clone());
    let format_id = saved
        .as_ref()
        .map(|f| f.format_id.clone())
        .or_else(|| previous.map(|p| p.format_id.clone()))
        .unwrap_or_else(|| record.format_id.clone());
    let extension = previous
        .map(|p| p.format_extension.clone())
        .unwrap_or_else(|| record.format_extension.clone());
    let height = previous.map(|p| p.height).unwrap_or(record.height);
    let fps = previous.map(|p| p.fps).unwrap_or(record.fps);
    let size = if previous.is_some() {
        record.file_size_bytes
    } else {
        record.selected_size_bytes
    };
    // Reconstructed command data is never persisted as a newly parsed page.
    let page = DownloadPageState {
        platform: record.platform.clone(),
        input_link: record.source_link.clone(),
        video_id: Some(record.video_id.clone()),
        title: Some(record.title.clone()),
        thumbnail_url: record.thumbnail_url.clone(),
        thumbnail_cache_path: record.thumbnail_cache_path.clone(),
        duration_seconds: record.duration_seconds,
        extension: extension.clone(),
        formats: vec![crate::video::VideoFormat {
            format_id: format_id.clone(),
            height,
            fps,
            extension,
            size_bytes: size,
            size_approximate: if previous.is_some() {
                size.is_some()
            } else {
                record.size_approximate
            },
            ..saved.unwrap_or_default()
        }],
        selected_format_id: Some(format_id),
        selected_height: height,
        selected_fps: fps,
        // A historical parser fallback must not suppress newly configured Cookie data.
        cookie_fallback: false,
        download_directory: previous
            .map(|p| p.directory.clone())
            .unwrap_or_else(|| record.download_directory.clone()),
        directory_customized: true,
        parser_fingerprint: Some("record".into()),
        parsed_at: Some(record.started_at.clone()),
        updated_at: String::new(),
    };
    page.validate("invalidDownloadOptions")
        .map_err(|e| error("invalidDownloadOptions", e.detail))?;
    Ok(page)
}

async fn capture_snapshot(
    mut page: DownloadPageState,
    tools: &RequiredToolManager,
    cookies: &CookieStore,
    storage: &Storage,
    recorded: bool,
) -> Result<TaskExecutionSnapshot, VideoError> {
    page.validate("invalidDownloadOptions")
        .map_err(|e| error("invalidDownloadOptions", e.detail))?;
    crate::video::formats::normalize_all(&mut page.formats);
    let platform: CookiePlatform = serde_json::from_value(serde_json::json!(page.platform))
        .map_err(|e| error("invalidDownloadOptions", e))?;
    let (settings, lease) = tools
        .settings_and_usage()
        .map_err(|e| error("toolSettingsFailed", e.detail))?;
    let fingerprint =
        crate::database::page_states::parser_fingerprint_for_platform(&settings, &page.platform)
            .map_err(|e| error("pageSaveFailed", e.detail))?;
    if recorded {
        page.parser_fingerprint = Some(fingerprint);
    } else if page.parser_fingerprint.as_ref() != Some(&fingerprint) {
        return Err(error(
            "invalidDownloadOptions",
            "Tool settings changed; parse the video again",
        ));
    }
    let proxy = storage
        .database()
        .and_then(|db| db.proxy_for_platform(platform))
        .map_err(|e| error("proxySettingsFailed", e.detail))?;
    let network_diagnostic = super::super::diagnostics::network_summary(&page.platform, proxy.as_ref());
    if matches!(platform, CookiePlatform::Douyin) {
        let contents = cookies
            .load(platform)
            .map_err(|_| error("cookieReadFailed", "Unable to read Douyin Cookie"))?;
        let ffprobe = settings
            .tools
            .get(&RequiredToolId::Ffmpeg)
            .and_then(|c| c.programs.iter().find(|p| p.name == "ffprobe"))
            .map(|p| p.path.clone())
            .ok_or_else(|| error("ffprobeMissing", "Configure ffprobe in Settings"))?;
        let runner = DownloadExecution::Douyin(
            crate::video::native_douyin::capture(
                &mut page,
                &contents,
                proxy.as_ref(),
                recorded,
                ffprobe,
            )
                .await?,
        );
        return Ok(TaskExecutionSnapshot {
            page,
            settings,
            platform,
            runner,
            network_diagnostic,
            command_options: None,
            _cookie: None,
            _tools: lease,
        });
    }
    let copy = if page.cookie_fallback {
        None
    } else {
        cookie_snapshot(cookies, platform)?
    };
    let options = DownloadCommandOptions {
        directory: page.download_directory.clone(),
        format_id: page
            .selected_format_id
            .clone()
            .ok_or_else(|| error("invalidDownloadOptions", "Missing format"))?,
        container: (page
            .formats
            .iter()
            .find(|f| Some(&f.format_id) == page.selected_format_id.as_ref())
            .and_then(|f| f.extension.as_deref())
            == Some("mp4"))
            .then(|| "mp4".into()),
        cookie_fallback: page.cookie_fallback,
        selected_format: page
            .formats
            .iter()
            .find(|format| Some(&format.format_id) == page.selected_format_id.as_ref())
            .cloned(),
    };
    let command = download_command(
        &settings,
        &normalize_link(&page.input_link, platform)?,
        copy.as_ref().map(|f| f.path()),
        &options,
        proxy.as_ref(),
    )?;
    let source = normalize_link(&page.input_link, platform)?;
    Ok(TaskExecutionSnapshot {
        page,
        settings,
        platform,
        runner: DownloadExecution::Ytdlp(command),
        network_diagnostic,
        command_options: Some((options, proxy, source)),
        _cookie: copy,
        _tools: lease,
    })
}

#[cfg(test)]
mod tests;
