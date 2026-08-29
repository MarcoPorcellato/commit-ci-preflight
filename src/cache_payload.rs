use crate::cache::CacheError;
#[cfg(test)]
use std::cell::{Cell, RefCell};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PayloadTreeStats {
    pub(crate) bytes: u64,
    pub(crate) files: u64,
}

pub(crate) fn measure_payload_tree(
    root: &Path,
    nodes: &mut usize,
    node_limit: usize,
) -> Result<PayloadTreeStats, CacheError> {
    let metadata = traced_symlink_metadata(root).map_err(CacheError::Io)?;
    if metadata.file_type().is_symlink() {
        return Err(CacheError::SymlinkInManagedRoot(root.to_path_buf()));
    }
    if !metadata.is_dir() {
        return Err(CacheError::UnexpectedEntry(root.to_path_buf()));
    }
    walk_payload(root, nodes, node_limit)
}

pub(crate) fn validate_payload_tree(
    root: &Path,
    nodes: &mut usize,
    node_limit: usize,
) -> Result<(), CacheError> {
    measure_payload_tree(root, nodes, node_limit).map(|_| ())
}

pub(crate) fn copy_payload_tree(
    source: &Path,
    destination: &Path,
    nodes: &mut usize,
    node_limit: usize,
) -> Result<(), CacheError> {
    let metadata = traced_symlink_metadata(source).map_err(CacheError::Io)?;
    if metadata.file_type().is_symlink() {
        return Err(CacheError::SymlinkInManagedRoot(source.to_path_buf()));
    }
    if !metadata.is_dir() {
        return Err(CacheError::UnexpectedEntry(source.to_path_buf()));
    }
    match traced_symlink_metadata(destination) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(CacheError::Io(error)),
        Ok(_) => return Err(CacheError::UnexpectedEntry(destination.to_path_buf())),
    }
    copy_payload_node(source, destination, nodes, node_limit)
}

fn copy_payload_node(
    source: &Path,
    destination: &Path,
    nodes: &mut usize,
    node_limit: usize,
) -> Result<(), CacheError> {
    *nodes = nodes
        .checked_add(1)
        .ok_or(CacheError::InventoryLimitExceeded)?;
    if *nodes > node_limit {
        return Err(CacheError::InventoryLimitExceeded);
    }

    let metadata = traced_symlink_metadata(source).map_err(CacheError::Io)?;
    if metadata.file_type().is_symlink() {
        return recreate_payload_link(source, destination);
    }
    if metadata.is_file() {
        traced_copy_file(source, destination).map_err(CacheError::Io)?;
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(CacheError::UnexpectedEntry(source.to_path_buf()));
    }

    traced_create_directory(destination).map_err(CacheError::Io)?;
    let mut entries = traced_read_directory(source)
        .map_err(CacheError::Io)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(CacheError::Io)?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        copy_payload_node(
            &entry.path(),
            &destination.join(entry.file_name()),
            nodes,
            node_limit,
        )?;
    }
    Ok(())
}

fn walk_payload(
    path: &Path,
    nodes: &mut usize,
    node_limit: usize,
) -> Result<PayloadTreeStats, CacheError> {
    *nodes = nodes
        .checked_add(1)
        .ok_or(CacheError::InventoryLimitExceeded)?;
    if *nodes > node_limit {
        return Err(CacheError::InventoryLimitExceeded);
    }

    let metadata = traced_symlink_metadata(path).map_err(CacheError::Io)?;
    if metadata.file_type().is_symlink() {
        return measure_symlink(path);
    }
    if metadata.is_file() {
        return Ok(PayloadTreeStats {
            bytes: metadata.len(),
            files: 1,
        });
    }
    if !metadata.is_dir() {
        return Err(CacheError::UnexpectedEntry(path.to_path_buf()));
    }

    let mut entries = traced_read_directory(path)
        .map_err(CacheError::Io)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(CacheError::Io)?;
    entries.sort_by_key(|e| e.file_name());

    let mut stats = PayloadTreeStats { bytes: 0, files: 0 };
    for entry in entries {
        let child = walk_payload(&entry.path(), nodes, node_limit)?;
        stats.bytes = stats
            .bytes
            .checked_add(child.bytes)
            .ok_or(CacheError::SizeOverflow)?;
        stats.files = stats
            .files
            .checked_add(child.files)
            .ok_or(CacheError::SizeOverflow)?;
    }
    Ok(stats)
}

#[cfg(unix)]
fn measure_symlink(path: &Path) -> Result<PayloadTreeStats, CacheError> {
    use std::os::unix::ffi::OsStrExt;

    let target = traced_read_link(path).map_err(|source| CacheError::PayloadSymlinkRead {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(PayloadTreeStats {
        bytes: target.as_os_str().as_bytes().len() as u64,
        files: 1,
    })
}

#[cfg(unix)]
fn recreate_payload_link(source: &Path, destination: &Path) -> Result<(), CacheError> {
    let target =
        traced_read_link(source).map_err(|source_error| CacheError::PayloadSymlinkRead {
            path: source.to_path_buf(),
            source: source_error,
        })?;
    match traced_symlink_metadata(destination) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(CacheError::Io(error)),
        Ok(_) => return Err(CacheError::UnexpectedEntry(destination.to_path_buf())),
    }
    traced_create_link(&target, destination).map_err(|source_error| {
        CacheError::PayloadSymlinkCreate {
            path: destination.to_path_buf(),
            source: source_error,
        }
    })
}

#[cfg(not(unix))]
fn recreate_payload_link(source: &Path, _destination: &Path) -> Result<(), CacheError> {
    Err(CacheError::PayloadSymlinkUnsupported(source.to_path_buf()))
}

#[cfg(not(unix))]
fn measure_symlink(path: &Path) -> Result<PayloadTreeStats, CacheError> {
    Err(CacheError::PayloadSymlinkUnsupported(path.to_path_buf()))
}

fn traced_symlink_metadata(path: &Path) -> std::io::Result<fs::Metadata> {
    #[cfg(test)]
    record_payload_operation(PayloadOperation::SymlinkMetadata(path.to_path_buf()));
    fs::symlink_metadata(path)
}

fn traced_read_directory(path: &Path) -> std::io::Result<fs::ReadDir> {
    #[cfg(test)]
    record_payload_operation(PayloadOperation::ReadDirectory(path.to_path_buf()));
    fs::read_dir(path)
}

fn traced_copy_file(source: &Path, destination: &Path) -> std::io::Result<u64> {
    #[cfg(test)]
    {
        record_payload_operation(PayloadOperation::CopyFile(destination.to_path_buf()));
        if take_payload_copy_failure_for_test() {
            return Err(io::Error::other("injected payload copy failure"));
        }
    }
    fs::copy(source, destination)
}

fn traced_create_directory(path: &Path) -> std::io::Result<()> {
    #[cfg(test)]
    record_payload_operation(PayloadOperation::CreateDirectory(path.to_path_buf()));
    fs::create_dir(path)
}

#[cfg(unix)]
fn traced_create_link(target: &Path, destination: &Path) -> std::io::Result<()> {
    #[cfg(test)]
    record_payload_operation(PayloadOperation::CreateLink(destination.to_path_buf()));
    std::os::unix::fs::symlink(target, destination)
}

#[cfg(unix)]
fn traced_read_link(path: &Path) -> std::io::Result<PathBuf> {
    #[cfg(test)]
    record_payload_operation(PayloadOperation::ReadLink(path.to_path_buf()));
    fs::read_link(path)
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
enum PayloadOperation {
    SymlinkMetadata(PathBuf),
    ReadDirectory(PathBuf),
    ReadLink(PathBuf),
    CopyFile(PathBuf),
    CreateDirectory(PathBuf),
    CreateLink(PathBuf),
}

#[cfg(test)]
impl PayloadOperation {
    fn filesystem_path(&self) -> &Path {
        match self {
            Self::SymlinkMetadata(path)
            | Self::ReadDirectory(path)
            | Self::ReadLink(path)
            | Self::CopyFile(path)
            | Self::CreateDirectory(path)
            | Self::CreateLink(path) => path,
        }
    }
}

#[cfg(test)]
thread_local! {
    static PAYLOAD_OPERATIONS: RefCell<Vec<PayloadOperation>> = const {
        RefCell::new(Vec::new())
    };
    static PAYLOAD_COPY_FAILURE: Cell<bool> = const { Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn fail_next_payload_copy_for_test() {
    PAYLOAD_COPY_FAILURE.with(|failure| failure.set(true));
}

#[cfg(test)]
pub(crate) fn clear_payload_copy_failure_for_test() {
    PAYLOAD_COPY_FAILURE.with(|failure| failure.set(false));
}

#[cfg(test)]
fn take_payload_copy_failure_for_test() -> bool {
    PAYLOAD_COPY_FAILURE.with(|failure| failure.replace(false))
}

#[cfg(test)]
fn record_payload_operation(operation: PayloadOperation) {
    PAYLOAD_OPERATIONS.with(|operations| operations.borrow_mut().push(operation));
}

#[cfg(test)]
fn clear_payload_operations() {
    PAYLOAD_OPERATIONS.with(|operations| operations.borrow_mut().clear());
}

#[cfg(test)]
fn take_payload_operations() -> Vec<PayloadOperation> {
    PAYLOAD_OPERATIONS.with(|operations| std::mem::take(&mut *operations.borrow_mut()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    #[cfg(unix)]
    const SHORT_SOCKET_DIRECTORY_RETRIES: usize = 16;

    #[cfg(unix)]
    struct ShortSocketFixtureDirectory {
        path: PathBuf,
    }

    #[cfg(unix)]
    impl ShortSocketFixtureDirectory {
        fn path(&self) -> &Path {
            &self.path
        }
    }

    #[cfg(unix)]
    impl Drop for ShortSocketFixtureDirectory {
        fn drop(&mut self) {
            match fs::symlink_metadata(&self.path) {
                Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                    let _ = fs::remove_dir_all(&self.path);
                }
                Ok(_) | Err(_) => {}
            }
        }
    }

    #[cfg(unix)]
    fn short_socket_fixture_directory() -> ShortSocketFixtureDirectory {
        let process_id = std::process::id();
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        for attempt in 0..SHORT_SOCKET_DIRECTORY_RETRIES {
            let path =
                PathBuf::from("/tmp").join(format!("ccp-ps-{process_id}-{sequence}-{attempt}"));
            match fs::create_dir(&path) {
                Ok(()) => return ShortSocketFixtureDirectory { path },
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create short socket fixture directory: {error}"),
            }
        }
        panic!("claim unique short socket fixture directory")
    }

    fn payload_fixture(name: &str) -> PathBuf {
        let base = std::env::var_os("CCP_TEST_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::current_dir()
                    .expect("current directory")
                    .parent()
                    .expect("repository parent")
                    .to_path_buf()
            });
        fs::create_dir_all(&base).expect("create payload fixture base");
        let path = base.join(format!(
            ".ccp-payload-test-{}-{}-{name}",
            std::process::id(),
            TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir(&path).expect("create payload fixture");
        path
    }

    fn remove_fixture(root: &Path, outside: &Path) {
        if root.exists() {
            fs::remove_dir_all(root).expect("remove payload fixture");
        }
        if outside.exists() {
            fs::remove_file(outside).expect("remove external sentinel");
        }
    }

    #[cfg(unix)]
    #[test]
    fn payload_measurement_counts_links_without_following_targets() {
        use std::os::unix::{ffi::OsStrExt, fs::symlink};

        let fixture = payload_fixture("measure-links");
        let outside = fixture
            .parent()
            .expect("fixture parent")
            .join("outside-sentinel");
        fs::write(&outside, b"do not read or change").expect("write external sentinel");
        fs::write(fixture.join("regular"), b"abc").expect("write regular file");
        symlink("regular", fixture.join("relative")).expect("create relative link");
        symlink("missing", fixture.join("broken")).expect("create broken link");
        symlink(&outside, fixture.join("absolute-external")).expect("create external link");
        symlink("self", fixture.join("self")).expect("create self link");
        symlink("cycle-b", fixture.join("cycle-a")).expect("create cycle-a link");
        symlink("cycle-a", fixture.join("cycle-b")).expect("create cycle-b link");
        fs::create_dir(fixture.join("nested")).expect("create nested directory");
        symlink("../relative", fixture.join("nested/recursive")).expect("create nested link");

        clear_payload_operations();
        let mut nodes = 0;
        let stats = measure_payload_tree(&fixture, &mut nodes, 100).expect("measure payload");
        let target_bytes = [
            "regular",
            "missing",
            "self",
            "cycle-b",
            "cycle-a",
            "../relative",
        ]
        .into_iter()
        .map(|target| target.len() as u64)
        .sum::<u64>()
            + outside.as_os_str().as_bytes().len() as u64;

        assert_eq!(stats.files, 8);
        assert_eq!(stats.bytes, 3 + target_bytes);
        assert_eq!(nodes, 10);
        assert_eq!(
            fs::read(&outside).expect("read external sentinel"),
            b"do not read or change"
        );
        assert!(
            take_payload_operations()
                .iter()
                .all(|operation| { !operation.filesystem_path().starts_with(&outside) })
        );
        remove_fixture(&fixture, &outside);
    }

    #[cfg(unix)]
    #[test]
    fn fallback_copy_preserves_each_link_target_and_external_sentinel() {
        use std::os::unix::fs::symlink;

        let source = payload_fixture("copy-source");
        let destination = source
            .parent()
            .expect("fixture parent")
            .join("copy-destination");
        let outside = source
            .parent()
            .expect("fixture parent")
            .join("copy-sentinel");
        fs::write(source.join("regular"), b"payload").expect("write regular payload");
        fs::write(&outside, b"outside").expect("write external sentinel");
        symlink("regular", source.join("relative")).expect("create relative link");
        symlink("missing", source.join("broken")).expect("create broken link");
        symlink(&outside, source.join("absolute")).expect("create absolute link");
        symlink("self", source.join("self")).expect("create self link");

        let mut nodes = 0;
        copy_payload_tree(&source, &destination, &mut nodes, 100).expect("copy payload");

        for name in ["relative", "broken", "absolute", "self"] {
            assert_eq!(
                fs::read_link(destination.join(name)).expect("read copied link"),
                fs::read_link(source.join(name)).expect("read source link")
            );
        }
        assert_eq!(
            fs::read(destination.join("regular")).expect("read copied payload"),
            b"payload"
        );
        assert_eq!(
            fs::read(&outside).expect("read external sentinel"),
            b"outside"
        );
        remove_fixture(&source, &outside);
        fs::remove_dir_all(destination).expect("remove copy destination");
    }

    #[cfg(unix)]
    #[test]
    fn fallback_copy_records_only_payload_paths_and_every_copy_operation() {
        use std::os::unix::fs::symlink;

        let source = payload_fixture("copy-trace-source");
        let destination = source
            .parent()
            .expect("fixture parent")
            .join("copy-trace-destination");
        let outside = source
            .parent()
            .expect("fixture parent")
            .join("copy-trace-sentinel");
        fs::create_dir(source.join("nested")).expect("create nested payload directory");
        fs::write(source.join("nested/regular"), b"payload").expect("write regular payload");
        fs::write(&outside, b"outside").expect("write external sentinel");
        symlink(&outside, source.join("nested/external")).expect("create external link");

        clear_payload_operations();
        let mut nodes = 0;
        copy_payload_tree(&source, &destination, &mut nodes, 100).expect("copy payload");
        let operations = take_payload_operations();

        assert!(
            operations
                .iter()
                .any(|operation| matches!(operation, PayloadOperation::CopyFile(_)))
        );
        assert!(
            operations
                .iter()
                .any(|operation| matches!(operation, PayloadOperation::CreateDirectory(_)))
        );
        assert!(
            operations
                .iter()
                .any(|operation| matches!(operation, PayloadOperation::CreateLink(_)))
        );
        assert!(
            operations
                .iter()
                .all(|operation| !operation.filesystem_path().starts_with(&outside))
        );

        remove_fixture(&source, &outside);
        fs::remove_dir_all(destination).expect("remove copy destination");
    }

    #[cfg(not(unix))]
    #[test]
    fn payload_link_recreation_is_explicitly_unsupported() {
        assert!(matches!(
            recreate_payload_link(Path::new("source"), Path::new("destination")),
            Err(CacheError::PayloadSymlinkUnsupported(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn payload_root_itself_must_be_a_plain_directory() {
        use std::os::unix::fs::symlink;

        let real = payload_fixture("plain-root-real");
        let link = real
            .parent()
            .expect("fixture parent")
            .join("plain-root-link");
        let _ = fs::remove_file(&link);
        symlink(&real, &link).expect("create root link");
        let mut nodes = 0;
        assert!(matches!(
            measure_payload_tree(&link, &mut nodes, 100),
            Err(CacheError::SymlinkInManagedRoot(path)) if path == link
        ));
        assert_eq!(nodes, 0);
        fs::remove_file(link).expect("remove root link");
        fs::remove_dir(real).expect("remove fixture");
    }

    #[cfg(unix)]
    #[test]
    fn short_socket_fixture_directory_removes_only_its_owned_path() {
        let directory = short_socket_fixture_directory();
        let path = directory.path().to_path_buf();
        let created = path.join("created-by-test");
        fs::write(&created, b"owned").expect("write owned fixture child");

        drop(directory);

        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn unsupported_payload_object_fails_without_traversal() {
        use std::os::unix::{fs::symlink, net::UnixListener};

        let fixture = payload_fixture("unsupported-object");
        let socket = fixture.join("listener.socket");
        let socket_directory = short_socket_fixture_directory();
        let socket_parent = socket_directory.path().join("fixture");
        symlink(&fixture, &socket_parent).expect("link short socket parent");
        let listener =
            UnixListener::bind(socket_parent.join("listener.socket")).expect("bind listener");
        clear_payload_operations();
        let mut nodes = 0;
        assert!(matches!(
            measure_payload_tree(&fixture, &mut nodes, 100),
            Err(CacheError::UnexpectedEntry(path)) if path == socket
        ));
        assert_eq!(nodes, 2);
        assert_eq!(
            take_payload_operations()
                .into_iter()
                .filter(|operation| operation.filesystem_path() == socket)
                .collect::<Vec<_>>(),
            vec![PayloadOperation::SymlinkMetadata(socket.clone())]
        );
        drop(listener);
        fs::remove_file(socket).expect("remove listener socket");
        drop(socket_directory);
        fs::remove_dir(fixture).expect("remove fixture");
    }

    #[test]
    fn payload_node_limit_is_fail_closed() {
        let fixture = payload_fixture("node-limit");
        fs::write(fixture.join("a"), b"a").expect("write first file");
        fs::write(fixture.join("b"), b"b").expect("write second file");
        let mut nodes = 0;
        assert!(matches!(
            measure_payload_tree(&fixture, &mut nodes, 2),
            Err(CacheError::InventoryLimitExceeded)
        ));
        assert_eq!(nodes, 3);
        fs::remove_dir_all(fixture).expect("remove fixture");
    }
}
