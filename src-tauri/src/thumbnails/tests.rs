use super::*;
use std::io::{Read, Write};

fn isolated_test_command(name: &str) -> std::process::Command {
    let mut child = std::process::Command::new(std::env::current_exe().unwrap());
    child.args([
        "--exact",
        &format!("thumbnails::tests::{name}"),
        "--nocapture",
    ]);
    for key in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "FTP_PROXY",
        "ftp_proxy",
        "NO_PROXY",
        "no_proxy",
    ] {
        child.env_remove(key);
    }
    child
}

fn run_direct_fixture(name: &str) -> bool {
    if std::env::var("EVD_TEST_DIRECT_THUMBNAIL_FIXTURE").as_deref() == Ok(name) {
        return false;
    }
    // Isolate only this child; leave the user's and parallel tests' environment untouched.
    let output = isolated_test_command(name)
        .env("NO_PROXY", "*")
        .env("no_proxy", "*")
        .env("EVD_TEST_DIRECT_THUMBNAIL_FIXTURE", name)
        .output()
        .unwrap();
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    true
}

#[tokio::test]
#[ignore = "child process fixture for isolated thumbnail proxy environment"]
async fn thumbnail_proxy_environment_fixture() {
    let dir = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary thumbnail proxy directory: {}",
        dir.path().display()
    );
    let store = ThumbnailStore::new(dir.path());
    let storage = Storage::new(&dir.path().join("app.db"), &dir.path().join("legacy.json"));
    let mode = std::env::var("EVD_TEST_THUMBNAIL_PROXY_MODE").unwrap();
    if mode != "missing" {
        let proxy = url::Url::parse(&std::env::var("EVD_TEST_THUMBNAIL_PROXY").unwrap()).unwrap();
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let closed_port = closed.local_addr().unwrap().port();
        drop(closed);
        storage
            .database()
            .unwrap()
            .save_proxy_settings(Some(&ProxySettings {
                protocol: "http".into(),
                address: "127.0.0.1".into(),
                port: if mode == "explicit" {
                    proxy.port().unwrap()
                } else {
                    closed_port
                },
            }))
            .unwrap();
        if mode == "explicit" {
            storage
                .database()
                .unwrap()
                .save_platform_settings(
                    crate::cookies::CookiePlatform::Youtube,
                    &crate::database::platform_settings::PlatformSettings {
                        proxy_enabled: true,
                    },
                )
                .unwrap();
        }
    }
    let result = store
        .cache_for_platform(
            "http://thumbnail-source.invalid/cover",
            crate::cookies::CookiePlatform::Youtube,
            &storage,
        )
        .await
        .and_then(|path| store.read(&path));
    drop(storage);
    dir.close().unwrap();
    let cached = result.expect("thumbnail should use the selected network proxy");
    assert_eq!(cached.mime, "image/png");
    assert_eq!(cached.bytes, b"\x89PNG\r\n\x1a\nproxy-image-fixture");
}

#[test]
fn thumbnail_download_follows_environment_or_explicit_platform_proxy() {
    for mode in ["missing", "disabled", "explicit"] {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy_url = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(15);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if std::time::Instant::now() >= deadline {
                            return None;
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("thumbnail proxy accept failed: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") && request.len() < 8192 {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            let payload = b"\x89PNG\r\n\x1a\nproxy-image-fixture";
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                payload.len()
            )
            .unwrap();
            stream.write_all(payload).unwrap();
            Some(String::from_utf8(request).unwrap())
        });
        let mut child = isolated_test_command("thumbnail_proxy_environment_fixture");
        child.arg("--ignored");
        child.env(
            "HTTP_PROXY",
            if mode == "explicit" {
                "http://127.0.0.1:0"
            } else {
                &proxy_url
            },
        );
        if mode == "explicit" {
            // Explicit app proxy must override both the environment proxy and bypass rules.
            child.env("NO_PROXY", "*").env("no_proxy", "*");
        }
        child
            .env("EVD_TEST_THUMBNAIL_PROXY_MODE", mode)
            .env("EVD_TEST_THUMBNAIL_PROXY", &proxy_url);
        let output = child.output().unwrap();
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
        let request = server.join().unwrap();
        assert!(
            output.status.success(),
            "mode={mode}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(request
            .unwrap()
            .starts_with("GET http://thumbnail-source.invalid/cover HTTP/1.1\r\n"));
    }
}

#[tokio::test]
async fn native_cache_reads_real_bytes_and_rejects_nonimages() {
    if run_direct_fixture("native_cache_reads_real_bytes_and_rejects_nonimages") {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary thumbnail test directory: {}",
        dir.path().display()
    );
    let store = ThumbnailStore::new(dir.path());
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/cover", socket.local_addr().unwrap());
    let payload = b"\x89PNG\r\n\x1a\nimage-fixture".to_vec();
    let expected = payload.clone();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = socket.accept().unwrap();
        let mut request = [0; 2048];
        assert!(stream.read(&mut request).unwrap() > 0);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            payload.len()
        )
            .unwrap();
        stream.write_all(&payload).unwrap();
    });
    let storage = Storage::new(&dir.path().join("app.db"), &dir.path().join("legacy.json"));
    let closed_proxy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = closed_proxy.local_addr().unwrap().port();
    drop(closed_proxy);
    let database = storage.database().unwrap();
    database.save_proxy_settings(Some(&ProxySettings { protocol: "http".into(), address: "127.0.0.1".into(), port })).unwrap();
    // A saved global proxy must not affect a platform whose proxy switch is off.
    let path = store.cache_for_platform(&url, crate::cookies::CookiePlatform::Youtube, &storage).await.unwrap();
    server.join().unwrap();
    assert!(path.starts_with("thumbnails/"));
    let actual = store.read(&path).unwrap();
    assert_eq!(actual.mime, "image/png");
    assert_eq!(actual.bytes, expected);
    database.save_platform_settings(crate::cookies::CookiePlatform::Youtube, &crate::database::platform_settings::PlatformSettings { proxy_enabled: true }).unwrap();
    assert!(store.cache_for_platform(&format!("{url}/uncached"), crate::cookies::CookiePlatform::Youtube, &storage).await.is_err());
    assert!(store.read("thumbnails/../outside").is_err());
    assert!(store.read("C:/Windows/file").is_err());
    assert!(image_mime(b"<html>not a cover</html>").is_none());
    let key = format!("thumbnails/{}.img", "a".repeat(64));
    std::fs::write(dir.path().join(&key), vec![0u8; MAX_BYTES + 1]).unwrap();
    assert!(store.read(&key).is_err());
}

#[tokio::test]
async fn rejected_http_and_image_responses_leave_no_cache_files() {
    if run_direct_fixture("rejected_http_and_image_responses_leave_no_cache_files") {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary thumbnail response directory: {}",
        dir.path().display()
    );
    let store = ThumbnailStore::new(dir.path());
    for (status, declared, payload) in [
        (404, 4, b"nope".as_slice()),
        (200, MAX_BYTES + 1, b"".as_slice()),
        (200, 6, b"<html>".as_slice()),
    ] {
        let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/cover", socket.local_addr().unwrap());
        let payload = payload.to_vec();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = socket.accept().unwrap();
            let mut request = [0; 2048];
            assert!(stream.read(&mut request).unwrap() > 0);
            write!(
                stream,
                "HTTP/1.1 {status} Test\r\nContent-Length: {declared}\r\nConnection: close\r\n\r\n"
            )
                .unwrap();
            stream.write_all(&payload).unwrap();
        });
        assert_eq!(
            store.cache(&url, None).await.unwrap_err().code,
            "thumbnailFailed"
        );
        server.join().unwrap();
    }
    assert_eq!(
        std::fs::read_dir(dir.path().join("thumbnails"))
            .unwrap()
            .count(),
        0
    );
}

#[tokio::test]
async fn thumbnail_timeout_does_not_write_a_partial_cache() {
    if run_direct_fixture("thumbnail_timeout_does_not_write_a_partial_cache") {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary thumbnail timeout directory: {}",
        dir.path().display()
    );
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/cover", socket.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (_stream, _) = socket.accept().unwrap();
        std::thread::sleep(Duration::from_secs(13));
    });
    let store = ThumbnailStore::new(dir.path());
    let started = std::time::Instant::now();
    assert!(store.cache(&url, None).await.is_err());
    assert!(started.elapsed() < Duration::from_secs(13));
    server.join().unwrap();
    assert_eq!(
        std::fs::read_dir(dir.path().join("thumbnails"))
            .unwrap()
            .count(),
        0
    );
}

#[cfg(unix)]
#[test]
fn thumbnail_symlink_outside_cache_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    eprintln!(
        "temporary thumbnail symlink directory: {}",
        dir.path().display()
    );
    let store = ThumbnailStore::new(dir.path());
    let root = store.root().unwrap();
    let outside = dir.path().join("outside.img");
    std::fs::write(&outside, b"\x89PNG\r\n\x1a\n").unwrap();
    let name = format!("{}.img", "a".repeat(64));
    std::os::unix::fs::symlink(outside, root.join(&name)).unwrap();
    assert!(store.read(&format!("thumbnails/{name}")).is_err());
}
