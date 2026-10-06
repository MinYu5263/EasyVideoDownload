use super::*;
use crate::database::persistence_tests::page;
use crate::video::download::history::permanent::delete_download_files;

fn fixture() -> (tempfile::TempDir, Database, i64, TemporaryDirectory) {
    let root = tempfile::Builder::new()
        .prefix("evd-owned-partials-")
        .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
        .unwrap();
    eprintln!("owned partial deletion fixture: {}", root.path().display());
    let home = root.path().canonicalize().unwrap();
    let db = Database::open(&home.join("app.db"), &home.join("legacy")).unwrap();
    let mut state = page();
    state.download_directory = home.to_string_lossy().into();
    let id = db
        .begin_download_record(
            "owner",
            &DownloadSnapshot { page: state },
            &crate::datetime::now(),
        )
        .unwrap();
    let temporary = db
        .prepare_download_temporary_directory("owner", &home)
        .unwrap();
    (root, db, id, temporary)
}
fn trash(db: &Database, id: i64) {
    db.finish_download_record("owner", &DownloadRecordOutcome::Paused)
        .unwrap();
    db.delete_download_record(id).unwrap();
}

#[test]
#[cfg(any(windows, target_os = "macos"))]
fn cancelling_removes_owned_fragments_and_record_but_keeps_siblings() {
    let (root, db, id, temporary) = fixture();
    std::fs::write(Path::new(&temporary.path).join("视频.part"), b"partial").unwrap();
    std::fs::write(Path::new(&temporary.path).join("video.ytdl"), b"resume").unwrap();
    let sibling = root.path().join("other-task.part");
    std::fs::write(&sibling, b"keep").unwrap();
    db.finish_download_record("owner", &DownloadRecordOutcome::Cancelled).unwrap();
    assert_eq!(db.remove_cancelled_download("owner").unwrap(), id);
    assert!(!Path::new(&temporary.path).exists());
    assert!(db.find_request_record("owner").unwrap().is_none());
    assert_eq!(std::fs::read(sibling).unwrap(), b"keep");
}

#[test]
#[cfg(target_os = "macos")]
fn a_link_inside_owned_fragments_does_not_delete_its_target_or_the_record() {
    let (root, db, id, temporary) = fixture();
    let outside = root.path().join("keep.mp4");
    std::fs::write(&outside, b"keep").unwrap();
    std::os::unix::fs::symlink(&outside, Path::new(&temporary.path).join("linked.part")).unwrap();
    db.finish_download_record("owner", &DownloadRecordOutcome::Cancelled).unwrap();
    assert_eq!(db.remove_cancelled_download("owner").unwrap_err().code, "cancelCleanupFailed");
    assert_eq!(std::fs::read(outside).unwrap(), b"keep");
    assert_eq!(db.get_download_record(id).unwrap().status, "cancelled");
}

#[test]
fn trash_and_restore_keep_partial_files_and_restart_reuses_the_persisted_directory() {
    let (root, db, id, temporary) = fixture();
    let partial = Path::new(&temporary.path).join("video.mp4.part");
    std::fs::write(&partial, b"resume these bytes").unwrap();
    trash(&db, id);
    assert_eq!(std::fs::read(&partial).unwrap(), b"resume these bytes");
    db.restore_download_record(id).unwrap();
    drop(db);
    let reopened =
        Database::open(&root.path().join("app.db"), &root.path().join("legacy")).unwrap();
    let mut state = page();
    state.download_directory = root.path().to_string_lossy().into();
    reopened
        .restart_download_record(
            "resumed",
            id,
            "owner",
            &DownloadSnapshot { page: state },
            false,
            |_, _| Ok(None),
        )
        .unwrap();
    reopened.mark_download_running("resumed").unwrap();
    assert_eq!(
        reopened
            .prepare_download_temporary_directory("resumed", root.path())
            .unwrap()
            .path,
        temporary.path
    );
    assert_eq!(std::fs::read(&partial).unwrap(), b"resume these bytes");
}

#[test]
#[cfg(any(windows, target_os = "macos"))]
fn permanent_delete_cleans_only_owned_partial_resume_fragment_and_intermediate_files() {
    let (root, db, id, temporary) = fixture();
    for name in [
        "video.mp4.part",
        "video.mp4.ytdl",
        "video.mp4.part-Frag12",
        "audio.m4a",
        "video.temp.mp4",
    ] {
        std::fs::write(Path::new(&temporary.path).join(name), b"owned").unwrap();
    }
    let unrelated = root.path().join("other-task.mp4.part");
    std::fs::write(&unrelated, b"keep unrelated").unwrap();
    let completed = root.path().join("other-video.mp4");
    std::fs::write(&completed, b"keep completed").unwrap();
    trash(&db, id);
    db.purge_download_record(id, delete_download_files).unwrap();
    assert!(!Path::new(&temporary.path).exists());
    assert_eq!(std::fs::read(unrelated).unwrap(), b"keep unrelated");
    assert_eq!(std::fs::read(completed).unwrap(), b"keep completed");
    assert_eq!(
        db.get_download_record(id).unwrap_err().code,
        "recordNotFound"
    );
}

#[test]
#[cfg(windows)]
fn empty_trash_keeps_occupied_record_and_removes_other_owned_downloads() {
    use std::os::windows::fs::OpenOptionsExt;
    let (root, db, id, temporary) = fixture();
    let partial = Path::new(&temporary.path).join("video.mp4.part");
    std::fs::write(&partial, b"occupied").unwrap();
    let occupied = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&partial)
        .unwrap();
    trash(&db, id);
    assert_eq!(
        db.purge_download_record(id, delete_download_files)
            .unwrap_err()
            .code,
        "historyFileOccupied"
    );
    assert!(db.get_download_record(id).unwrap().deleted_at.is_some());
    let mut state = page();
    state.video_id = Some("second-video".into());
    state.download_directory = root.path().to_string_lossy().into();
    let second = db
        .begin_download_record(
            "second",
            &DownloadSnapshot { page: state },
            &crate::datetime::now(),
        )
        .unwrap();
    let second_dir = db
        .prepare_download_temporary_directory("second", root.path())
        .unwrap();
    std::fs::write(Path::new(&second_dir.path).join("audio.part"), b"second").unwrap();
    db.finish_download_record("second", &DownloadRecordOutcome::Paused)
        .unwrap();
    db.delete_download_record(second).unwrap();
    assert_eq!(
        db.empty_download_record_trash(delete_download_files)
            .unwrap_err()
            .code,
        "historyTrashPartiallyDeleted"
    );
    assert!(!Path::new(&second_dir.path).exists());
    assert!(db.get_download_record(id).unwrap().deleted_at.is_some());
    drop(occupied);
    db.empty_download_record_trash(delete_download_files)
        .unwrap();
    assert!(!Path::new(&temporary.path).exists());
}

#[test]
fn replaced_temporary_directory_is_never_adopted_or_deleted() {
    let (root, db, id, temporary) = fixture();
    let moved = root.path().join("moved-owned-directory");
    std::fs::rename(&temporary.path, &moved).unwrap();
    std::fs::create_dir(&temporary.path).unwrap();
    let replacement = Path::new(&temporary.path).join("unrelated.part");
    std::fs::write(&replacement, b"replacement").unwrap();
    assert_eq!(
        db.prepare_download_temporary_directory("owner", root.path())
            .unwrap_err()
            .code,
        "historyFileChanged"
    );
    trash(&db, id);
    assert_eq!(
        db.purge_download_record(id, delete_download_files)
            .unwrap_err()
            .code,
        "historyFileChanged"
    );
    assert_eq!(std::fs::read(&replacement).unwrap(), b"replacement");
    assert!(db.get_download_record(id).unwrap().deleted_at.is_some());
}

#[test]
#[cfg(windows)]
fn a_junction_replacement_cannot_redirect_cleanup_to_other_files() {
    let (root, db, id, temporary) = fixture();
    let moved = root.path().join("moved original");
    let outside = root.path().join("other task files");
    std::fs::rename(&temporary.path, &moved).unwrap();
    std::fs::create_dir(&outside).unwrap();
    let other = outside.join("keep.part");
    std::fs::write(&other, b"keep other task").unwrap();
    let junction = std::process::Command::new("cmd.exe")
        .args(["/c", "mklink", "/J"])
        .arg(&temporary.path)
        .arg(&outside)
        .output()
        .unwrap();
    assert!(
        junction.status.success(),
        "{}",
        String::from_utf8_lossy(&junction.stderr)
    );
    trash(&db, id);
    assert_eq!(
        db.purge_download_record(id, delete_download_files)
            .unwrap_err()
            .code,
        "historyFileUnsafe"
    );
    assert_eq!(std::fs::read(&other).unwrap(), b"keep other task");
    // Remove exactly this fixture's junction, before TempDir cleans its owned root.
    std::fs::remove_dir(&temporary.path).unwrap();
}

#[test]
#[cfg(any(windows, target_os = "macos"))]
fn failed_ownership_save_cleans_only_the_new_empty_directory() {
    let (root, db, _id, temporary) = fixture();
    remove_empty(&temporary).unwrap();
    let unrelated = root.path().join("keep.part");
    std::fs::write(&unrelated, b"keep").unwrap();
    db.connection("test").unwrap().execute_batch("CREATE TRIGGER reject_temporary_owner BEFORE INSERT ON download_temporary_directories BEGIN SELECT RAISE(ABORT,'synthetic ownership write failure'); END;").unwrap();
    assert_eq!(
        db.prepare_download_temporary_directory("owner", root.path())
            .unwrap_err()
            .code,
        "saveFailed"
    );
    assert_eq!(std::fs::read(unrelated).unwrap(), b"keep");
    assert!(!std::fs::read_dir(root.path()).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".evd-")));
}

#[test]
#[cfg(windows)]
fn an_active_download_reserving_an_owned_directory_blocks_permanent_cleanup() {
    let (root, db, id, temporary) = fixture();
    let partial = Path::new(&temporary.path).join("keep.part");
    std::fs::write(&partial, b"keep").unwrap();
    trash(&db, id);
    let mut state = page();
    state.video_id = Some("active-other-video".into());
    state.download_directory = temporary.path.clone();
    db.begin_download_record(
        "active-other",
        &DownloadSnapshot { page: state },
        &crate::datetime::now(),
    )
        .unwrap();
    assert_eq!(
        db.purge_download_record(id, delete_download_files)
            .unwrap_err()
            .code,
        "historyBusy"
    );
    assert_eq!(std::fs::read(partial).unwrap(), b"keep");
    assert!(db.get_download_record(id).unwrap().deleted_at.is_some());
    assert!(root.path().exists());
}

#[test]
#[cfg(windows)]
fn a_queued_download_in_a_missing_descendant_reserves_the_owned_directory() {
    let (_root, db, id, temporary) = fixture();
    trash(&db, id);
    let ordinary_root = temporary
        .path
        .strip_prefix(r"\\?\")
        .unwrap_or(&temporary.path);
    let pending_home = Path::new(ordinary_root).join("not created yet");
    assert!(!pending_home.exists());
    let mut state = page();
    state.video_id = Some("queued-descendant-video".into());
    state.download_directory = pending_home.to_string_lossy().into();
    db.accept_download_record(
        "queued-descendant",
        &DownloadSnapshot { page: state },
        false,
        false,
    )
        .unwrap();
    assert_eq!(
        db.purge_download_record(id, delete_download_files)
            .unwrap_err()
            .code,
        "historyBusy"
    );
    assert!(Path::new(&temporary.path).exists());
}

#[test]
#[cfg(any(windows, target_os = "macos"))]
fn unexpected_subdirectory_stops_cleanup_without_deleting_any_files() {
    let (_root, db, id, temporary) = fixture();
    let partial = Path::new(&temporary.path).join("video.part");
    std::fs::write(&partial, b"keep").unwrap();
    std::fs::create_dir(Path::new(&temporary.path).join("unexpected")).unwrap();
    trash(&db, id);
    assert_eq!(
        db.purge_download_record(id, delete_download_files)
            .unwrap_err()
            .code,
        "historyFileUnsafe"
    );
    assert_eq!(std::fs::read(partial).unwrap(), b"keep");
}

#[test]
#[cfg(any(windows, target_os = "macos"))]
fn missing_directory_is_idempotent_and_unknown_legacy_partials_are_kept() {
    let (root, db, id, temporary) = fixture();
    std::fs::remove_dir(&temporary.path).unwrap();
    let legacy = root.path().join("video.mp4.part");
    std::fs::write(&legacy, b"legacy").unwrap();
    trash(&db, id);
    db.purge_download_record(id, delete_download_files).unwrap();
    assert_eq!(std::fs::read(legacy).unwrap(), b"legacy");
}

#[test]
#[cfg(any(windows, target_os = "macos"))]
fn a_final_output_referenced_by_another_record_is_protected_inside_a_temporary_directory() {
    let (root, db, id, temporary) = fixture();
    let output = Path::new(&temporary.path).join("referenced.mp4");
    std::fs::write(&output, b"keep shared output").unwrap();
    let mut state = page();
    state.video_id = Some("other-final-video".into());
    state.download_directory = root.path().to_string_lossy().into();
    db.begin_download_record(
        "other-output",
        &DownloadSnapshot { page: state },
        &crate::datetime::now(),
    )
        .unwrap();
    db.finish_download_record(
        "other-output",
        &DownloadRecordOutcome::Completed {
            path: output.to_string_lossy().into(),
            size: 18,
            extension: Some("mp4".into()),
        },
    )
        .unwrap();
    trash(&db, id);
    assert_eq!(
        db.purge_download_record(id, delete_download_files)
            .unwrap_err()
            .code,
        "historyFileInUse"
    );
    assert_eq!(std::fs::read(output).unwrap(), b"keep shared output");
}

#[test]
#[cfg(any(windows, target_os = "macos"))]
fn all_old_directories_are_cleaned_after_changing_the_download_directory() {
    let (root, db, id, original) = fixture();
    std::fs::write(Path::new(&original.path).join("old.part"), b"old").unwrap();
    let new_home = root.path().join("new-downloads");
    std::fs::create_dir(&new_home).unwrap();
    let newer = db
        .prepare_download_temporary_directory("owner", &new_home)
        .unwrap();
    std::fs::write(Path::new(&newer.path).join("new.part"), b"new").unwrap();
    assert_ne!(original.path, newer.path);
    trash(&db, id);
    db.purge_download_record(id, delete_download_files).unwrap();
    assert!(!Path::new(&original.path).exists());
    assert!(!Path::new(&newer.path).exists());
    assert!(new_home.is_dir());
}
