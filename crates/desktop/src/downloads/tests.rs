use super::*;
#[test]
fn names_cannot_escape_downloads_or_use_windows_devices() {
    assert_eq!(safe_filename("../../hello.txt"), "hello.txt");
    assert_eq!(safe_filename("C:\\folder\\report?.txt"), "report_.txt");
    assert_eq!(safe_filename(".."), "download");
    assert_eq!(safe_filename("CON.txt"), "_CON.txt");
    assert_eq!(safe_filename(" hello.txt. "), "hello.txt");
}
#[test]
fn publication_never_overwrites_and_numbers_before_extension() {
    let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("report.txt"), b"existing").unwrap();
    let temporary = root.join("partial");
    std::fs::write(&temporary, b"download").unwrap();
    let mut transfer = Transfer {
        suggested: "report.txt".into(),
        item: DownloadItem {
            id: "test".into(),
            filename: "report.txt".into(),
            path: root.join("report.txt"),
            received: 0,
            total: None,
            status: DownloadStatus::Downloading,
            error: None,
        },
        temporary,
        native: None,
    };
    finish(&mut transfer, Ok(()));
    assert_eq!(transfer.item.status, DownloadStatus::Completed);
    assert_eq!(transfer.item.filename, "report (1).txt");
    assert_eq!(std::fs::read(root.join("report.txt")).unwrap(), b"existing");
    assert_eq!(std::fs::read(&transfer.item.path).unwrap(), b"download");
    let copied = root.join("copy.txt");
    copy_new_file(&transfer.item.path, &copied).unwrap();
    assert_eq!(std::fs::read(&copied).unwrap(), b"download");
    assert_eq!(
        copy_new_file(&transfer.item.path, &root.join("report.txt"))
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::AlreadyExists
    );
    assert_eq!(std::fs::read(root.join("report.txt")).unwrap(), b"existing");
    assert!(!transfer.temporary.exists());
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn reservations_cover_concurrent_same_name_and_failures_clean_partials() {
    let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&root).unwrap();
    let reserved = HashSet::from([root.join("file.zip")]);
    assert_eq!(
        available_path(&root, "file.zip", &reserved).unwrap(),
        root.join("file (1).zip")
    );
    let temporary = root.join("partial");
    std::fs::write(&temporary, b"partial").unwrap();
    let mut transfer = Transfer {
        suggested: "file.zip".into(),
        item: DownloadItem {
            id: "test".into(),
            filename: "file.zip".into(),
            path: root.join("file.zip"),
            received: 3,
            total: None,
            status: DownloadStatus::Downloading,
            error: None,
        },
        temporary,
        native: None,
    };
    finish(&mut transfer, Err("Connection lost".into()));
    assert_eq!(transfer.item.status, DownloadStatus::Failed);
    assert!(!transfer.temporary.exists());
    assert!(!transfer.item.path.exists());
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn publication_respects_active_names_and_external_collisions() {
    let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&root).unwrap();
    let temporary = root.join("partial");
    std::fs::write(&temporary, b"download").unwrap();
    std::fs::write(root.join("report (1).txt"), b"external").unwrap();
    let mut transfer = Transfer {
        suggested: "report.txt".into(),
        temporary,
        item: DownloadItem {
            id: "test".into(),
            filename: "report (1).txt".into(),
            path: root.join("report (1).txt"),
            received: 0,
            total: None,
            status: DownloadStatus::Downloading,
            error: None,
        },
        native: None,
    };
    finish_reserved(
        &mut transfer,
        Ok(()),
        &HashSet::from([root.join("report.txt")]),
    );
    assert_eq!(transfer.item.filename, "report (2).txt");
    assert_eq!(
        std::fs::read(root.join("report (1).txt")).unwrap(),
        b"external"
    );
    assert!(!root.join("report.txt").exists());
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn cancellation_is_idempotent_and_drop_cleans_owned_partials() {
    let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&root).unwrap();
    let partial = root.join("partial");
    let item = DownloadItem {
        id: "test".into(),
        filename: "file.txt".into(),
        path: root.join("file.txt"),
        received: 0,
        total: None,
        status: DownloadStatus::Downloading,
        error: None,
    };
    let mut manager = DownloadManager::default();
    std::fs::write(&partial, b"partial").unwrap();
    manager.transfers.push(Transfer {
        item: item.clone(),
        suggested: "file.txt".into(),
        temporary: partial.clone(),
        native: None,
    });
    manager.cancel("test").unwrap();
    manager.cancel("test").unwrap();
    assert_eq!(manager.items()[0].status, DownloadStatus::Cancelled);
    assert!(!partial.exists());
    assert!(manager.open("test").is_err());
    std::fs::write(&partial, b"partial").unwrap();
    drop(manager);
    assert!(!partial.exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn dangling_symlink_is_a_filename_collision() {
    let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(root.join("missing"), root.join("file.txt")).unwrap();
    assert_eq!(
        available_path(&root, "file.txt", &HashSet::new()).unwrap(),
        root.join("file (1).txt")
    );
    assert!(
        std::fs::symlink_metadata(root.join("file.txt"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn copy_fallback_preserves_native_file_metadata() {
    use std::ffi::CString;
    let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&root).unwrap();
    let source = root.join("source");
    let destination = root.join("destination");
    std::fs::write(&source, b"download").unwrap();
    let attribute = CString::new("com.axolotl.download-test").unwrap();
    let value = b"native metadata";
    let source_name = CString::new(source.to_str().unwrap()).unwrap();
    assert_eq!(
        unsafe {
            libc::setxattr(
                source_name.as_ptr(),
                attribute.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
                0,
            )
        },
        0
    );
    copy_new_file(&source, &destination).unwrap();
    let destination_name = CString::new(destination.to_str().unwrap()).unwrap();
    let mut actual = [0u8; 64];
    let size = unsafe {
        libc::getxattr(
            destination_name.as_ptr(),
            attribute.as_ptr(),
            actual.as_mut_ptr().cast(),
            actual.len(),
            0,
            0,
        )
    };
    assert_eq!(size, value.len() as isize);
    assert_eq!(&actual[..size as usize], value);
    std::fs::remove_dir_all(root).unwrap();
}
