use super::*;
use std::fs;

fn store() -> (tempfile::TempDir, CookieStore) {
    let dir = tempfile::Builder::new()
        .prefix("cookie-storage-")
        .tempdir()
        .unwrap();
    eprintln!("Cookie test temporary directory: {}", dir.path().display());
    let store = CookieStore::new(&dir.path().join("data"));
    (dir, store)
}

#[test]
fn absent_cookie_reads_as_empty_without_creating_files() {
    let (_dir, store) = store();
    assert_eq!(store.load(CookiePlatform::Douyin).unwrap(), "");
    assert!(!store.directory.exists());
}

#[test]
fn each_platform_creates_its_exact_managed_file() {
    let (_dir, store) = store();
    for (platform, name, text) in [
        (CookiePlatform::Douyin, "douyin_cookies.txt", "douyin text"),
        (
            CookiePlatform::Bilibili,
            "bilibili_cookies.txt",
            "bilibili text",
        ),
        (
            CookiePlatform::Youtube,
            "youtube_cookies.txt",
            "youtube text",
        ),
    ] {
        store.save(platform, text).unwrap();
        assert_eq!(
            fs::read_to_string(store.directory.join(name)).unwrap(),
            text
        );
    }
    assert_eq!(fs::read_dir(&store.directory).unwrap().count(), 3);
}

#[test]
fn edits_replace_contents_and_a_new_store_restores_exact_text() {
    let (dir, store) = store();
    store
        .save(CookiePlatform::Bilibili, "old contents")
        .unwrap();
    let contents = "# Demonstration only\r\n任意文本\twithout format validation\r\n";
    store.save(CookiePlatform::Bilibili, contents).unwrap();
    let reopened = CookieStore::new(&dir.path().join("data"));
    assert_eq!(reopened.load(CookiePlatform::Bilibili).unwrap(), contents);
    assert_eq!(fs::read_dir(&store.directory).unwrap().count(), 1);
}

#[test]
fn clearing_removes_only_the_current_platform_and_is_repeatable() {
    let (_dir, store) = store();
    store
        .save(CookiePlatform::Douyin, "douyin contents")
        .unwrap();
    store
        .save(CookiePlatform::Youtube, "youtube contents")
        .unwrap();
    store.save(CookiePlatform::Douyin, "").unwrap();
    store.save(CookiePlatform::Douyin, "").unwrap();
    assert!(!store.path(CookiePlatform::Douyin).exists());
    assert_eq!(store.load(CookiePlatform::Douyin).unwrap(), "");
    assert_eq!(
        store.load(CookiePlatform::Youtube).unwrap(),
        "youtube contents"
    );
}

#[test]
fn directory_creation_failure_is_reported_without_overwriting_the_blocking_file() {
    let (dir, store) = store();
    fs::write(dir.path().join("data"), "existing data").unwrap();
    assert_eq!(
        store
            .save(CookiePlatform::Douyin, "new contents")
            .unwrap_err()
            .code,
        "saveFailed"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("data")).unwrap(),
        "existing data"
    );
}

#[test]
fn invalid_utf8_is_a_read_failure_instead_of_an_empty_configuration() {
    let (_dir, store) = store();
    fs::create_dir_all(&store.directory).unwrap();
    fs::write(store.path(CookiePlatform::Douyin), [0xff, 0xfe]).unwrap();
    assert_eq!(
        store.load(CookiePlatform::Douyin).unwrap_err().code,
        "loadFailed"
    );
    assert_eq!(
        fs::read(store.path(CookiePlatform::Douyin)).unwrap(),
        [0xff, 0xfe]
    );
}

#[test]
fn failed_replacement_keeps_the_target_and_cleans_its_temporary_file() {
    let (_dir, store) = store();
    let target = store.path(CookiePlatform::Bilibili);
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("keep.txt"), "keep").unwrap();
    assert_eq!(
        store
            .save(CookiePlatform::Bilibili, "new contents")
            .unwrap_err()
            .code,
        "saveFailed"
    );
    assert_eq!(fs::read_to_string(target.join("keep.txt")).unwrap(), "keep");
    assert_eq!(fs::read_dir(&store.directory).unwrap().count(), 1);
}

#[test]
fn only_known_platform_identifiers_are_accepted() {
    for value in ["../douyin", "Douyin", "other", "youtube/../../app.db"] {
        assert!(serde_json::from_value::<CookiePlatform>(serde_json::json!(value)).is_err());
    }
}
