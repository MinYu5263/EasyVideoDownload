use super::*;
use crate::database::{platform_settings::PlatformSettings, Database};
use crate::required_tools::{Program, RequiredToolConfig, RequiredToolSource};
use std::sync::{Arc, Mutex};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

async fn local_server(
    body: &'static [u8],
) -> (u16, Arc<Mutex<Vec<String>>>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let recorded = requests.clone();
    let task = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let recorded = recorded.clone();
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                let mut buffer = [0; 4096];
                while bytes.len() < 16384 && !bytes.windows(4).any(|tail| tail == b"\r\n\r\n") {
                    let read = stream.read(&mut buffer).await.unwrap();
                    if read == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&buffer[..read]);
                }
                let request = String::from_utf8_lossy(&bytes)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string();
                recorded.lock().unwrap().push(request.clone());
                let header = format!("HTTP/1.1 200 OK\r\nContent-Type: video/mp4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                stream.write_all(header.as_bytes()).await.unwrap();
                if !request.starts_with("HEAD ") {
                    stream.write_all(body).await.unwrap();
                }
            });
        }
    });
    (port, requests, task)
}

fn with_network_environment(command: &mut Command, inherited_proxy: Option<&str>) {
    // Simulate the parent network environment only on this child, then apply
    // the app's original overrides. Other tests and the user's environment stay untouched.
    let overrides: Vec<_> = command
        .as_std()
        .get_envs()
        .map(|(name, value)| (name.to_owned(), value.map(ToOwned::to_owned)))
        .collect();
    for name in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        if let Some(url) = inherited_proxy {
            command.env(name, url);
        } else {
            command.env_remove(name);
        }
    }
    command.env_remove("FTP_PROXY").env_remove("ftp_proxy");
    if inherited_proxy.is_some() {
        // An absent bypass is stable across Windows PowerShell's environment refresh;
        // an empty value can become null after launching a native executable.
        command.env_remove("NO_PROXY").env_remove("no_proxy");
    } else {
        command.env("NO_PROXY", "*").env("no_proxy", "*");
    }
    for (name, value) in overrides {
        if let Some(value) = value {
            command.env(name, value);
        } else {
            command.env_remove(name);
        }
    }
}

#[tokio::test]
#[ignore = "local native smoke test; set EVD_PROXY_TEST_YTDLP and EVD_PROXY_TEST_FFMPEG to installed executables"]
async fn real_ytdlp_parsing_and_downloading_follow_the_persisted_platform_switch() {
    let ytdlp = std::env::var_os("EVD_PROXY_TEST_YTDLP").expect("installed yt-dlp path");
    let ffmpeg = std::env::var_os("EVD_PROXY_TEST_FFMPEG").expect("installed FFmpeg path");
    let mut tools = RequiredToolSettings::default();
    for (id, name, path) in [
        (
            RequiredToolId::Ytdlp,
            "yt-dlp",
            std::path::PathBuf::from(ytdlp),
        ),
        (
            RequiredToolId::Ffmpeg,
            "ffmpeg",
            std::path::PathBuf::from(ffmpeg),
        ),
    ] {
        tools.tools.insert(
            id,
            RequiredToolConfig {
                source: RequiredToolSource::Manual,
                manual_path: path.to_string_lossy().into(),
                programs: vec![Program {
                    name: name.into(),
                    path,
                    version: "local smoke test".into(),
                }],
                checked_at: crate::datetime::now(),
            },
        );
    }
    let directory = tempfile::tempdir().unwrap();
    eprintln!(
        "native proxy smoke test directory: {}",
        directory.path().display()
    );
    let database = Database::open(
        &directory.path().join("app.db"),
        &directory.path().join("legacy.json"),
    )
        .unwrap();
    let (origin_port, origin_requests, origin) = local_server(b"direct-video-bytes").await;
    let (inherited_port, inherited_requests, inherited) =
        local_server(b"default-proxy-video-bytes").await;
    let (proxy_port, proxy_requests, proxy) = local_server(b"proxied-video-bytes").await;
    database
        .save_proxy_settings(Some(&ProxySettings {
            protocol: "http".into(),
            address: "127.0.0.1".into(),
            port: proxy_port,
        }))
        .unwrap();
    let url = format!("http://127.0.0.1:{origin_port}/probe.mp4");
    let inherited_url = format!("http://127.0.0.1:{inherited_port}");
    for (attempt, (use_inherited, enabled)) in
        [(false, false), (true, false), (true, true), (true, false)]
            .into_iter()
            .enumerate()
    {
        let network_proxy = use_inherited.then_some(inherited_url.as_str());
        origin_requests.lock().unwrap().clear();
        inherited_requests.lock().unwrap().clear();
        proxy_requests.lock().unwrap().clear();
        database
            .save_platform_settings(
                CookiePlatform::Youtube,
                &PlatformSettings {
                    proxy_enabled: enabled,
                },
            )
            .unwrap();
        let snapshot = database
            .proxy_for_platform(CookiePlatform::Youtube)
            .unwrap();
        let mut parse = parsing_command(&tools, &url, None, snapshot.as_ref()).unwrap();
        with_network_environment(&mut parse, network_proxy);
        parse.kill_on_drop(true);
        let parsed = tokio::time::timeout(Duration::from_secs(30), parse.output())
            .await
            .unwrap()
            .unwrap();
        assert!(
            parsed.status.success(),
            "{}",
            String::from_utf8_lossy(&parsed.stderr)
        );
        let metadata: serde_json::Value = serde_json::from_slice(&parsed.stdout).unwrap();
        #[cfg(windows)]
        {
            let preview = commands::render_command(&parse, true).unwrap();
            let script = format!("$beforeHttp = [Environment]::GetEnvironmentVariable('HTTP_PROXY', 'Process'); $beforeNo = [Environment]::GetEnvironmentVariable('NO_PROXY', 'Process'); {}; $afterHttp = [Environment]::GetEnvironmentVariable('HTTP_PROXY', 'Process'); $afterNo = [Environment]::GetEnvironmentVariable('NO_PROXY', 'Process'); if ($afterHttp -cne $beforeHttp -or $afterNo -cne $beforeNo) {{ throw \"Proxy environment changed: HTTP before=$beforeHttp after=$afterHttp; NO_PROXY before=$beforeNo after=$afterNo\" }}; exit $LASTEXITCODE", preview.text);
            let mut shell = Command::new("powershell.exe");
            shell
                .args(["-NoProfile", "-NonInteractive", "-Command", &script])
                .kill_on_drop(true);
            // Verify CLI arguments in the same child environment as the app;
            // the preview itself does not expand environment setup into a script.
            for (name, value) in parse.as_std().get_envs() {
                if let Some(value) = value {
                    shell.env(name, value);
                } else {
                    shell.env_remove(name);
                }
            }
            let shown = tokio::time::timeout(Duration::from_secs(30), shell.output())
                .await
                .unwrap()
                .unwrap();
            assert!(
                shown.status.success(),
                "attempt {attempt}, inherited={use_inherited}, enabled={enabled}: {}",
                String::from_utf8_lossy(&shown.stderr)
            );
            serde_json::from_slice::<serde_json::Value>(&shown.stdout)
                .expect("the copied PowerShell command returns real metadata");
        }
        let format_id = metadata["formats"][0]["format_id"].as_str().unwrap();
        let output = directory.path().join(format!("output-{attempt}-{enabled}"));
        let options: DownloadCommandOptions = serde_json::from_value(serde_json::json!({
            "directory": output, "formatId": format_id,
        }))
            .unwrap();
        let mut download =
            commands::download_command(&tools, &url, None, &options, snapshot.as_ref()).unwrap();
        with_network_environment(&mut download, network_proxy);
        download.kill_on_drop(true);
        let downloaded = tokio::time::timeout(Duration::from_secs(30), download.output())
            .await
            .unwrap()
            .unwrap();
        assert!(
            downloaded.status.success(),
            "{}",
            String::from_utf8_lossy(&downloaded.stderr)
        );
        let text = String::from_utf8_lossy(&downloaded.stdout);
        let report: serde_json::Value = serde_json::from_str(
            text.lines()
                .find_map(|line| line.strip_prefix("__EVD_FILE__"))
                .expect("native file confirmation"),
        )
            .unwrap();
        let bytes = std::fs::read(report["filepath"].as_str().unwrap()).unwrap();
        assert_eq!(
            bytes,
            if enabled {
                b"proxied-video-bytes".as_slice()
            } else if use_inherited {
                b"default-proxy-video-bytes".as_slice()
            } else {
                b"direct-video-bytes".as_slice()
            }
        );
        if enabled {
            assert!(origin_requests.lock().unwrap().is_empty());
            assert!(inherited_requests.lock().unwrap().is_empty());
            assert!(proxy_requests
                .lock()
                .unwrap()
                .iter()
                .all(|line| line.contains(&url)));
            assert!(proxy_requests.lock().unwrap().len() >= 2);
        } else if use_inherited {
            assert!(origin_requests.lock().unwrap().is_empty());
            assert!(proxy_requests.lock().unwrap().is_empty());
            assert!(inherited_requests
                .lock()
                .unwrap()
                .iter()
                .all(|line| line.contains(&url)));
            assert!(inherited_requests.lock().unwrap().len() >= 2);
        } else {
            assert!(inherited_requests.lock().unwrap().is_empty());
            assert!(proxy_requests.lock().unwrap().is_empty());
            assert!(origin_requests.lock().unwrap().len() >= 2);
        }
        assert!(database
            .proxy_for_platform(CookiePlatform::Douyin)
            .unwrap()
            .is_none());
    }
    origin.abort();
    inherited.abort();
    proxy.abort();
}
