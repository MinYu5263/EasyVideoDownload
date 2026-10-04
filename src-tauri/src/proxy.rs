use crate::database::{Storage, StorageError};
use serde::{Deserialize, Serialize};
use std::{
    error::Error as _,
    time::{Duration, Instant},
};

const TEST_URL: &str = "https://www.youtube.com/robots.txt";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyTestResult {
    pub target: &'static str,
    pub status: u16,
    pub elapsed_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct ProxyTestError {
    pub code: String,
    pub detail: String,
}

fn test_error(code: &str, detail: impl ToString) -> ProxyTestError {
    ProxyTestError {
        code: code.into(),
        detail: detail.to_string(),
    }
}

fn request_error(error: reqwest::Error) -> ProxyTestError {
    if error.is_timeout() {
        return test_error("timeout", "");
    }
    let mut source = error.source();
    let mut text = error.to_string();
    while let Some(cause) = source {
        if cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|e| e.kind() == std::io::ErrorKind::ConnectionRefused)
        {
            return test_error("connectionRefused", "");
        }
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    let text = text.to_ascii_lowercase();
    let code = if [
        "proxy authentication required",
        "proxy authorization required",
        "server does not support user/pass authentication",
    ]
        .iter()
        .any(|message| text.contains(message))
    {
        "proxyAuthRequired"
    } else if ["certificate", "tls", "ssl", "schannel"]
        .iter()
        .any(|word| text.contains(word))
    {
        "tlsFailed"
    } else if error.is_connect() {
        "proxyConnectionFailed"
    } else {
        "requestFailed"
    };
    test_error(code, "")
}

async fn probe(
    settings: &ProxySettings,
    target: &str,
    limit: Duration,
) -> Result<ProxyTestResult, ProxyTestError> {
    let settings = settings
        .normalized()
        .map_err(|_| test_error("invalidSettings", ""))?;
    let proxy = reqwest::Proxy::all(settings.url())
        .map_err(|_| test_error("invalidSettings", ""))?
        .no_proxy(None);
    // Explicit proxy only: environment exclusions and system proxy settings cannot bypass it.
    let client = reqwest::Client::builder()
        .no_proxy()
        .proxy(proxy)
        .connect_timeout(Duration::from_secs(5).min(limit))
        .timeout(limit)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("EasyVideoDownload/0.1")
        .build()
        .map_err(|_| test_error("testUnavailable", ""))?;
    let started = Instant::now();
    let mut response = client.get(target).send().await.map_err(request_error)?;
    let status = response.status();
    if status.as_u16() == 407 {
        return Err(test_error("proxyAuthRequired", "HTTP 407"));
    }
    if !status.is_success() {
        return Err(test_error(
            "targetHttpError",
            format!("HTTP {}", status.as_u16()),
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > 65536)
    {
        return Err(test_error("responseTooLarge", ""));
    }
    let mut bytes = 0;
    while let Some(chunk) = response.chunk().await.map_err(request_error)? {
        bytes += chunk.len();
        if bytes > 65536 {
            return Err(test_error("responseTooLarge", ""));
        }
    }
    Ok(ProxyTestResult {
        target: "YouTube",
        status: status.as_u16(),
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

#[tauri::command]
pub async fn test_proxy_connection(
    settings: ProxySettings,
) -> Result<ProxyTestResult, ProxyTestError> {
    probe(&settings, TEST_URL, Duration::from_secs(10)).await
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProxySettings {
    pub protocol: String,
    pub address: String,
    pub port: u16,
}

impl ProxySettings {
    pub fn normalized(&self) -> Result<Self, StorageError> {
        let invalid =
            || StorageError::new("invalidSettings", "Invalid proxy protocol, address or port");
        if !matches!(self.protocol.as_str(), "http" | "https" | "socks5") || self.port == 0 {
            return Err(invalid());
        }
        let address = self.address.trim();
        if address.is_empty()
            || address
            .chars()
            .any(|c| c.is_whitespace() || "/\\?#@%".contains(c))
        {
            return Err(invalid());
        }
        let host_text = if address.contains(':') && !address.starts_with('[') {
            format!("[{address}]")
        } else {
            address.into()
        };
        let address = match url::Host::parse(&host_text).map_err(|_| invalid())? {
            url::Host::Domain(domain) => {
                let name = domain.strip_suffix('.').unwrap_or(&domain);
                if name.len() > 253
                    || name.split('.').any(|label| {
                    label.is_empty()
                        || label.len() > 63
                        || label.starts_with('-')
                        || label.ends_with('-')
                        || !label
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'-')
                })
                {
                    return Err(invalid());
                }
                domain
            }
            url::Host::Ipv4(address) => address.to_string(),
            url::Host::Ipv6(address) => address.to_string(),
        };
        Ok(Self {
            protocol: self.protocol.clone(),
            address,
            port: self.port,
        })
    }

    pub fn url(&self) -> String {
        let scheme = if self.protocol == "socks5" {
            "socks5h"
        } else {
            &self.protocol
        };
        let host = if self.address.contains(':') {
            format!("[{}]", self.address)
        } else {
            self.address.clone()
        };
        format!("{scheme}://{host}:{}", self.port)
    }
}

pub async fn saved_proxy(storage: &Storage) -> Result<Option<ProxySettings>, StorageError> {
    let database = storage.database()?;
    tauri::async_runtime::spawn_blocking(move || database.proxy_settings())
        .await
        .map_err(|e| StorageError::new("loadFailed", e))?
}

pub async fn platform_proxy(
    storage: &Storage,
    platform: crate::cookies::CookiePlatform,
) -> Result<Option<ProxySettings>, StorageError> {
    let database = storage.database()?;
    tauri::async_runtime::spawn_blocking(move || database.proxy_for_platform(platform))
        .await
        .map_err(|e| StorageError::new("loadFailed", e))?
}

#[tauri::command]
pub async fn get_proxy_settings(
    state: tauri::State<'_, Storage>,
) -> Result<Option<ProxySettings>, StorageError> {
    saved_proxy(state.inner()).await
}

#[tauri::command]
pub async fn save_proxy_settings(
    settings: Option<ProxySettings>,
    state: tauri::State<'_, Storage>,
) -> Result<Option<ProxySettings>, StorageError> {
    let settings = settings.map(|settings| settings.normalized()).transpose()?;
    let database = state.database()?;
    tauri::async_runtime::spawn_blocking(move || {
        database.save_proxy_settings(settings.as_ref())?;
        Ok(settings)
    })
        .await
        .map_err(|e| StorageError::new("saveFailed", e))?
}

#[cfg(test)]
mod tests;
