use super::{LabError, ParsedResult};
use reqwest::{cookie::CookieStore, Client, RequestBuilder};
use serde_json::Value;
use std::sync::Arc;
use url::Url;

pub(super) const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36";
const DETAIL_ENDPOINT: &str = "https://www.douyin.com/aweme/v1/web/aweme/detail/";

pub(crate) struct DouyinClient {
    http: Client,
    jar: Arc<reqwest::cookie::Jar>,
}
impl std::ops::Deref for DouyinClient {
    type Target = Client;
    fn deref(&self) -> &Client {
        &self.http
    }
}
pub(crate) fn client(
    contents: &str,
    proxy: Option<&crate::proxy::ProxySettings>,
) -> Result<DouyinClient, LabError> {
    let jar = super::cookies::cookie_jar(contents)?;
    let mut builder = Client::builder()
        .no_proxy()
        .cookie_provider(jar.clone())
        .user_agent(UA)
        .connect_timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            // Visitor/signing headers belong only to the detail endpoint. Never
            // forward them to a redirect target (including a media CDN).
            if attempt
                .previous()
                .first()
                .is_some_and(|u| u.path() == "/aweme/v1/web/aweme/detail/")
            {
                attempt.stop()
            } else if attempt.previous().len() >= 8 {
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
        .map(|http| DouyinClient { http, jar })
        .map_err(|_| LabError::new("networkFailed", "Unable to create HTTP client"))
}

// Douyin's web SDK binds the visitor and current timestamp to the exact encoded
// query. Keep the same bytes for hashing and sending; never log this request.
fn detail_request(
    client: &DouyinClient,
    id: &str,
    timestamp: i64,
) -> Result<RequestBuilder, LabError> {
    let mut endpoint = Url::parse(DETAIL_ENDPOINT).unwrap();
    let cookies = client.jar.cookies(&endpoint);
    let cookies = cookies
        .as_ref()
        .and_then(|header| header.to_str().ok())
        .unwrap_or("");
    let value = |name: &str| {
        cookies
            .split(';')
            .filter_map(|pair| pair.trim().split_once('='))
            .find(|(key, value)| *key == name && !value.is_empty())
            .map(|(_, value)| value)
    };
    let visitor = [
        "uifid",
        "uifid_temp",
        "uifidtemp",
        "UIFID",
        "UIFID_TEMP",
        "UIFIDTEMP",
    ]
    .into_iter()
    .find_map(value)
    .ok_or_else(|| {
        LabError::new(
            "cookieMissing",
            "Platform visitor identity is unavailable; reimport a fresh browser export",
        )
    })?;
    endpoint.query_pairs_mut().extend_pairs([
        ("aweme_id", id),
        ("aid", "6383"),
        ("channel", "channel_pc_web"),
        ("detail_list", "1"),
    ]);
    if let Some(fp) = value("s_v_web_id") {
        endpoint
            .query_pairs_mut()
            .extend_pairs([("verifyFp", fp), ("fp", fp)]);
    }
    let stamp = timestamp.to_string();
    endpoint
        .query_pairs_mut()
        .extend_pairs([("uifid", visitor), ("timestamp", stamp.as_str())]);
    let signature = format!(
        "{:x}",
        md5::compute(format!(
            "{visitor}_{stamp}_A96D855A08C0A9707F8BEF0D9A527E4E_{}",
            endpoint.query().unwrap()
        ))
    );
    endpoint
        .query_pairs_mut()
        .append_pair("x-secsdk-web-signature", &signature);
    let sensitive = |text: &str| {
        let mut header = reqwest::header::HeaderValue::from_str(text).map_err(|_| {
            LabError::new(
                "cookieMissing",
                "Platform visitor identity is invalid; reimport a fresh browser export",
            )
        })?;
        header.set_sensitive(true);
        Ok::<_, LabError>(header)
    };
    Ok(client
        .get(endpoint)
        .header("uifid", sensitive(visitor)?)
        .header("x-secsdk-web-signature", sensitive(&signature)?)
        .header("x-secsdk-web-expire", stamp)
        .header("Accept", "application/json, text/plain, */*"))
}

async fn request_json(request: RequestBuilder) -> Result<Value, LabError> {
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        let mut response = request
            .header("Referer", "https://www.douyin.com/")
            .send()
            .await
            .map_err(|_| LabError::new("networkFailed", "Detail request failed"))?;
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let mut prefix = Vec::new();
            let _ = tokio::time::timeout(std::time::Duration::from_secs(2), async {
                while prefix.len() < 1024 {
                    let Some(chunk) = response.chunk().await? else {
                        break;
                    };
                    prefix.extend_from_slice(&chunk[..chunk.len().min(1024 - prefix.len())]);
                }
                Ok::<_, reqwest::Error>(())
            })
            .await;
            // Only known static gateway reasons may leave this boundary. Never
            // expose an arbitrary response body, URL, identity or signed query.
            let body = String::from_utf8_lossy(&prefix);
            let reason = if body.contains("ArgusSecurityPlugin") {
                if body.contains("Uifid Not Found") {
                    "missing_client_identity"
                } else if body.contains("Signature Not Found") {
                    "missing_request_proof"
                } else if body.contains("Sign Invalid") {
                    "invalid_request_proof"
                } else if body.contains("Sign Expired") {
                    "expired_request_proof"
                } else {
                    "request_rejected"
                }
            } else {
                "unknown"
            };
            return Err(LabError::new(
                "httpFailed",
                &format!("Detail request was rejected (HTTP {status}; gateway={reason})"),
            ));
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

pub(crate) async fn parse_video(
    client: &DouyinClient,
    input: &str,
) -> Result<ParsedResult, LabError> {
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

pub(super) async fn fetch_detail(
    client: &DouyinClient,
    id: &str,
) -> Result<ParsedResult, LabError> {
    let request = detail_request(client, id, chrono::Utc::now().timestamp())?;
    let result = super::parse_detail(&request_json(request).await?)?;
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
        server_with_status("200 OK", body)
    }
    fn server_with_status(
        status: &'static str,
        body: &'static str,
    ) -> (Url, std::thread::JoinHandle<String>) {
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
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
            String::from_utf8_lossy(&buf[..n]).into_owned()
        });
        (url, task)
    }
    #[tokio::test]
    async fn rejected_detail_preserves_status_and_safe_gateway_reason() {
        let client = Client::builder().no_proxy().build().unwrap();
        for (body, reason) in [
            (
                "Blocked by ArgusSecurityPlugin Uifid Not Found",
                "missing_client_identity",
            ),
            (
                "Blocked by ArgusSecurityPlugin Signature Not Found",
                "missing_request_proof",
            ),
            (
                "Cookie: session=private-value https://media.test/?token=private-token",
                "unknown",
            ),
        ] {
            let (url, task) = server_with_status("403 Forbidden", body);
            let error = request_json(client.get(url)).await.unwrap_err();
            task.join().unwrap();
            assert_eq!(error.code, "httpFailed");
            assert!(error.detail.contains("HTTP 403"), "{}", error.detail);
            assert!(error.detail.contains(reason), "{}", error.detail);
            assert!(!error.detail.contains("private"));
            assert_eq!(crate::app_logs::safe_text(&error.detail), error.detail);
        }
    }
    #[test]
    fn detail_request_binds_the_wire_query_to_scoped_visitor_identity() {
        let client = client(".douyin.com\tTRUE\t/\tTRUE\t0\tUIFID\tvisitor+one\n.douyin.com\tTRUE\t/\tTRUE\t0\ts_v_web_id\tverify/a=b\n", None).unwrap();
        let request = detail_request(&client, "123", 1_700_000_000)
            .unwrap()
            .build()
            .unwrap();
        let covered = "aweme_id=123&aid=6383&channel=channel_pc_web&detail_list=1&verifyFp=verify%2Fa%3Db&fp=verify%2Fa%3Db&uifid=visitor%2Bone&timestamp=1700000000";
        assert_eq!(
            request.url().query().unwrap(),
            format!("{covered}&x-secsdk-web-signature=4aa09cdcbb71860c98674a453d490309")
        );
        assert_eq!(request.headers()["uifid"], "visitor+one");
        assert_eq!(
            request.headers()["x-secsdk-web-signature"],
            "4aa09cdcbb71860c98674a453d490309"
        );
        assert_eq!(request.headers()["x-secsdk-web-expire"], "1700000000");
        let media = client.get("https://media.test/video").build().unwrap();
        assert!(!media.headers().contains_key("uifid"));
        assert!(!media.headers().contains_key("x-secsdk-web-signature"));
    }
    #[test]
    fn visitor_identity_uses_sdk_aliases_and_rejects_out_of_scope_values() {
        for name in [
            "uifid",
            "uifid_temp",
            "uifidtemp",
            "UIFID",
            "UIFID_TEMP",
            "UIFIDTEMP",
        ] {
            let contents = format!(".douyin.com\tTRUE\t/\tTRUE\t0\t{name}\tvisitor\n");
            let client = client(&contents, None).unwrap();
            let request = detail_request(&client, "123", 1_700_000_000)
                .unwrap()
                .build()
                .unwrap();
            assert_eq!(request.headers()["uifid"], "visitor");
        }
        let client = client(".douyin.com\tTRUE\t/\tTRUE\t0\tsession\tpresent\nv.douyin.com\tFALSE\t/\tTRUE\t0\tUIFID\twrong-host\n.douyin.com\tTRUE\t/video\tTRUE\t0\tUIFID_TEMP\twrong-path\n.douyin.com\tTRUE\t/\tTRUE\t1\tuifid\texpired\n", None).unwrap();
        let error = detail_request(&client, "123", 1_700_000_000).err().unwrap();
        assert_eq!(error.code, "cookieMissing");
        assert!(!error.detail.contains("wrong-"));
        assert!(!error.detail.contains("expired"));
    }
    #[tokio::test]
    async fn signed_detail_does_not_follow_redirects_with_visitor_headers() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = Url::parse(&format!(
            "http://{}/aweme/v1/web/aweme/detail/",
            listener.local_addr().unwrap()
        ))
        .unwrap();
        let task = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut bytes = [0; 4096];
            stream.read(&mut bytes).unwrap();
            stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: https://127.0.0.1:1/other\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        });
        let client = client(".douyin.com\tTRUE\t/\tTRUE\t0\tUIFID\tvisitor\n", None).unwrap();
        let mut request = detail_request(&client, "123", 1_700_000_000)
            .unwrap()
            .build()
            .unwrap();
        // Route only this synthetic-identity request through a real local server.
        *request.url_mut() = url;
        let response = client.execute(request).await.unwrap();
        task.join().unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::FOUND);
    }
    #[tokio::test]
    async fn reads_real_json_response_and_rejects_empty_body() {
        let client = Client::builder().no_proxy().build().unwrap();
        let (url, task) = server("{\"status_code\":0}");
        let result = request_json(client.get(url)).await;
        // Join only after the request: no fake transport response.
        if result.is_err() {
            drop(task);
        } else {
            task.join().unwrap();
        }
        assert_eq!(result.unwrap()["status_code"], 0);
        let (url, task) = server("");
        assert_eq!(
            request_json(client.get(url)).await.unwrap_err().code,
            "emptyResponse"
        );
        task.join().unwrap();
    }
    #[tokio::test]
    #[ignore = "requires an explicitly authorized Cookie file and live Douyin network"]
    async fn live_reported_short_link_parses_and_refreshes_with_native_requests() {
        let path = std::env::var("DOUYIN_TEST_COOKIE_FILE").expect("authorized file path required");
        let contents = std::fs::read_to_string(path).unwrap();
        let metadata = crate::video::native_douyin::parse(
            &contents,
            None,
            "https://v.douyin.com/dE68GdQy5gk/",
        )
        .await
        .unwrap();
        assert_eq!(metadata.id, "7657872802652900614");
        assert!(!metadata.formats.is_empty());
        assert!(metadata
            .formats
            .iter()
            .any(|f| Some(&f.format_id) == metadata.default_format_id.as_ref()));
        let client = client(&contents, None).unwrap();
        let refreshed = fetch_detail(&client, "7657872802652900614").await.unwrap();
        assert_eq!(refreshed.video.video_id, metadata.id);
        assert!(!refreshed.candidates.is_empty());
        eprintln!(
            "Verified native parse: {} formats; native refresh: {} formats",
            metadata.formats.len(),
            refreshed.candidates.len()
        );
    }
}
