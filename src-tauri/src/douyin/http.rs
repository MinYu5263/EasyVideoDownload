use super::{LabError, ParsedResult};
use reqwest::Client;
use serde_json::Value;
use url::Url;

pub(super) const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36";
pub(super) fn client(
    contents: &str,
    proxy: Option<&crate::proxy::ProxySettings>,
) -> Result<Client, LabError> {
    let jar = super::cookies::cookie_jar(contents)?;
    let mut builder = Client::builder()
        .no_proxy()
        .cookie_provider(jar)
        .user_agent(UA)
        .connect_timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 8 {
                attempt.error("Redirect limit exceeded")
            } else if attempt.url().scheme() == "https" {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }));
    if let Some(proxy) = proxy {
        builder = builder.proxy(
            reqwest::Proxy::all(proxy.url())
                .map_err(|_| LabError::new("proxyFailed", "Invalid proxy configuration"))?,
        );
    }
    builder
        .build()
        .map_err(|_| LabError::new("networkFailed", "Unable to create HTTP client"))
}

async fn request_json(client: &Client, url: Url) -> Result<Value, LabError> {
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        let mut response = client
            .get(url)
            .header("Referer", "https://www.douyin.com/")
            .send()
            .await
            .map_err(|_| LabError::new("networkFailed", "Detail request failed"))?;
        if !response.status().is_success() {
            return Err(LabError::new("httpFailed", "Detail request was rejected"));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| LabError::new("networkFailed", "Detail response interrupted"))?
        {
            if bytes.len() + chunk.len() > 6 * 1024 * 1024 {
                return Err(LabError::new(
                    "invalidResponse",
                    "Detail response exceeds limit",
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        if bytes.iter().all(u8::is_ascii_whitespace) {
            return Err(LabError::new(
                "emptyResponse",
                "No detail returned; import fresh Douyin cookies",
            ));
        }
        serde_json::from_slice(&bytes)
            .map_err(|_| LabError::new("invalidResponse", "Detail response is not JSON"))
    })
        .await
        .map_err(|_| LabError::new("timeout", "Detail request timed out"))?
}

pub(super) async fn parse_video(client: &Client, input: &str) -> Result<ParsedResult, LabError> {
    let url = super::normalize_link(input)?;
    let url = if url.host_str() == Some("v.douyin.com") {
        tokio::time::timeout(std::time::Duration::from_secs(30), client.get(url).send())
            .await
            .map_err(|_| LabError::new("timeout", "Short link resolution timed out"))?
            .map_err(|_| LabError::new("networkFailed", "Short link resolution failed"))?
            .url()
            .clone()
    } else {
        url
    };
    let url = super::normalize_link(url.as_str())?;
    let segments: Vec<_> = url.path_segments().into_iter().flatten().collect();
    let id = segments
        .windows(2)
        .find(|s| {
            s[0] == "video"
                && !s[1].is_empty()
                && s[1].len() <= 24
                && s[1].bytes().all(|b| b.is_ascii_digit())
        })
        .map(|s| s[1])
        .ok_or_else(|| LabError::new("unsupported", "Expected a single video link"))?;
    fetch_detail(client, id).await
}

pub(super) async fn fetch_detail(client: &Client, id: &str) -> Result<ParsedResult, LabError> {
    let mut endpoint = Url::parse("https://www.douyin.com/aweme/v1/web/aweme/detail/").unwrap();
    endpoint.query_pairs_mut().extend_pairs([
        ("aweme_id", id),
        ("aid", "6383"),
        ("channel", "channel_pc_web"),
        ("detail_list", "1"),
    ]);
    let result = super::parse_detail(&request_json(client, endpoint).await?)?;
    if result.video.video_id != id {
        return Err(LabError::new(
            "invalidResponse",
            "Returned video identity does not match",
        ));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    fn server(body: &'static str) -> (Url, std::thread::JoinHandle<String>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = Url::parse(&format!("http://{}/detail", listener.local_addr().unwrap())).unwrap();
        let task = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut buf = [0; 4096];
            let n = stream.read(&mut buf).unwrap();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
                .unwrap();
            String::from_utf8_lossy(&buf[..n]).into_owned()
        });
        (url, task)
    }
    #[tokio::test]
    async fn reads_real_json_response_and_rejects_empty_body() {
        let client = Client::builder().no_proxy().build().unwrap();
        let (url, task) = server("{\"status_code\":0}");
        let result = request_json(&client, url).await;
        // Join only after the request: no fake transport response.
        if result.is_err() {
            drop(task);
        } else {
            task.join().unwrap();
        }
        assert_eq!(result.unwrap()["status_code"], 0);
        let (url, task) = server("");
        assert_eq!(
            request_json(&client, url).await.unwrap_err().code,
            "emptyResponse"
        );
        task.join().unwrap();
    }
}
