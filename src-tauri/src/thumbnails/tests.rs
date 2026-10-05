use super::*;
use std::io::{Read, Write};

#[tokio::test]
async fn native_cache_reads_real_bytes_and_rejects_nonimages() {
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
