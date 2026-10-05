use super::LabError;
use std::sync::Arc;

pub(super) fn cookie_jar(contents: &str) -> Result<Arc<reqwest::cookie::Jar>, LabError> {
    use reqwest::cookie::CookieStore;
    let jar = Arc::new(reqwest::cookie::Jar::default());
    let now = chrono::Utc::now().timestamp();
    for original in contents.lines() {
        let line = original.strip_prefix("#HttpOnly_").unwrap_or(original);
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let fields: Vec<_> = line.splitn(7, '\t').collect();
        if fields.len() != 7 {
            continue;
        }
        let domain = fields[0].trim_start_matches('.');
        if !super::parse::douyin_host(domain) {
            continue;
        }
        let Ok(expiry) = fields[4].parse::<i64>() else {
            continue;
        };
        if expiry < 0 || (expiry > 0 && expiry <= now) {
            continue;
        }
        if fields[5].is_empty()
            || !fields[2].starts_with('/')
            || fields.iter().any(|v| v.contains(['\r', '\n', ';']))
        {
            continue;
        }
        let Ok(origin) = url::Url::parse(&format!("https://{domain}/")) else {
            continue;
        };
        let mut cookie = format!("{}={}; Path={}", fields[5], fields[6], fields[2]);
        if fields[1].eq_ignore_ascii_case("TRUE") {
            cookie.push_str(&format!("; Domain={domain}"));
        }
        if fields[3].eq_ignore_ascii_case("TRUE") {
            cookie.push_str("; Secure");
        }
        if expiry > 0 {
            cookie.push_str(&format!("; Max-Age={}", expiry - now));
        }
        if original.starts_with("#HttpOnly_") {
            cookie.push_str("; HttpOnly");
        }
        jar.add_cookie_str(&cookie, &origin);
    }
    if jar
        .cookies(&url::Url::parse("https://www.douyin.com/aweme/v1/web/aweme/detail/").unwrap())
        .is_none()
    {
        return Err(LabError::new(
            "cookieMissing",
            "Import fresh Douyin cookies on the download page",
        ));
    }
    Ok(jar)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::cookie::CookieStore;
    #[test]
    fn imported_cookies_obey_scope_and_expiry() {
        let jar = cookie_jar("# Netscape HTTP Cookie File\n.douyin.com\tTRUE\t/\tTRUE\t0\twide\tone\nwww.douyin.com\tFALSE\t/video\tFALSE\t0\thost\ttwo\n.douyin.com\tTRUE\t/\tFALSE\t1\told\tthree\n.evil.test\tTRUE\t/\tFALSE\t0\tevil\tfour\n").unwrap();
        let cookies = |s: &str| {
            jar.cookies(&url::Url::parse(s).unwrap())
                .map(|v| v.to_str().unwrap().to_string())
                .unwrap_or_default()
        };
        assert!(cookies("https://www.douyin.com/video/1").contains("host=two"));
        assert!(cookies("https://v.douyin.com/").contains("wide=one"));
        assert!(!cookies("https://v.douyin.com/video/1").contains("host="));
        assert!(!cookies("https://www.douyin.com/other").contains("host="));
        assert!(!cookies("http://www.douyin.com/").contains("wide="));
        assert!(!cookies("https://www.douyin.com/").contains("old="));
        assert!(cookies("https://evil.test/").is_empty());
    }
}
