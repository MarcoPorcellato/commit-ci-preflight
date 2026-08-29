use std::fs;
use std::path::{Path, PathBuf};
use crate::cache::CacheError;
#[cfg(test)] use std::cell::RefCell;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PayloadTreeStats { pub(crate) bytes: u64, pub(crate) files: u64 }

pub(crate) fn measure_payload_tree(root: &Path, nodes: &mut usize, node_limit: usize) -> Result<PayloadTreeStats, CacheError> {
    let metadata = traced_symlink_metadata(root)?;
    if metadata.file_type().is_symlink() { return Err(CacheError::SymlinkInManagedRoot(root.to_path_buf())); }
    if !metadata.is_dir() { return Err(CacheError::UnexpectedEntry(root.to_path_buf())); }
    walk_payload(root, nodes, node_limit)
}
pub(crate) fn validate_payload_tree(root: &Path, nodes: &mut usize, node_limit: usize) -> Result<(), CacheError> { measure_payload_tree(root, nodes, node_limit).map(|_| ()) }

fn walk_payload(root: &Path, nodes: &mut usize, limit: usize) -> Result<PayloadTreeStats, CacheError> {
    *nodes = nodes.checked_add(1).ok_or(CacheError::InventoryLimitExceeded)?;
    if *nodes > limit { return Err(CacheError::InventoryLimitExceeded); }
    let mut entries: Vec<_> = traced_read_directory(root)?.collect::<Result<Vec<_>, _>>().map_err(|_| CacheError::UnexpectedEntry(root.to_path_buf()))?;
    entries.sort_by_key(|e| e.file_name());
    let mut out = PayloadTreeStats { bytes: 0, files: 0 };
    for entry in entries { *nodes = nodes.checked_add(1).ok_or(CacheError::InventoryLimitExceeded)?; if *nodes > limit { return Err(CacheError::InventoryLimitExceeded); } let path = entry.path(); let metadata = traced_symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            let target = traced_read_link(&path).map_err(|source| CacheError::PayloadSymlinkRead { path: path.clone(), source })?;
            #[cfg(unix)] { use std::os::unix::ffi::OsStrExt; out.bytes = out.bytes.checked_add(target.as_os_str().as_bytes().len() as u64).ok_or(CacheError::SizeOverflow)?; }
            #[cfg(not(unix))] { return Err(CacheError::PayloadSymlinkUnsupported(path)); }
            out.files = out.files.checked_add(1).ok_or(CacheError::SizeOverflow)?;
        } else if metadata.is_dir() { let child = walk_payload(&path, nodes, limit)?; out.bytes = out.bytes.checked_add(child.bytes).ok_or(CacheError::SizeOverflow)?; out.files = out.files.checked_add(child.files).ok_or(CacheError::SizeOverflow)?;
        } else if metadata.is_file() { out.bytes = out.bytes.checked_add(metadata.len()).ok_or(CacheError::SizeOverflow)?; out.files = out.files.checked_add(1).ok_or(CacheError::SizeOverflow)?;
        } else { return Err(CacheError::UnexpectedEntry(path)); }
    } Ok(out)
}
fn traced_symlink_metadata(path: &Path) -> Result<fs::Metadata, CacheError> { #[cfg(test)] record_payload_operation(PayloadOperation::SymlinkMetadata(path.to_path_buf())); fs::symlink_metadata(path).map_err(|_| CacheError::UnexpectedEntry(path.to_path_buf())) }
fn traced_read_directory(path: &Path) -> Result<fs::ReadDir, CacheError> { #[cfg(test)] record_payload_operation(PayloadOperation::ReadDirectory(path.to_path_buf())); fs::read_dir(path).map_err(|_| CacheError::UnexpectedEntry(path.to_path_buf())) }
fn traced_read_link(path: &Path) -> Result<PathBuf, std::io::Error> { #[cfg(test)] record_payload_operation(PayloadOperation::ReadLink(path.to_path_buf())); fs::read_link(path) }

#[cfg(test)] #[derive(Debug, Clone, PartialEq, Eq)] enum PayloadOperation { SymlinkMetadata(PathBuf), ReadDirectory(PathBuf), ReadLink(PathBuf) }
#[cfg(test)] impl PayloadOperation { fn filesystem_path(&self)->&Path { match self { Self::SymlinkMetadata(p)|Self::ReadDirectory(p)|Self::ReadLink(p)=>p } } }
#[cfg(test)] thread_local! { static PAYLOAD_OPERATIONS: RefCell<Vec<PayloadOperation>> = const { RefCell::new(Vec::new()) }; }
#[cfg(test)] fn record_payload_operation(op: PayloadOperation) { PAYLOAD_OPERATIONS.with(|v| v.borrow_mut().push(op)); }
#[cfg(test)] fn clear_payload_operations() { PAYLOAD_OPERATIONS.with(|v| v.borrow_mut().clear()); }
#[cfg(test)] fn take_payload_operations() -> Vec<PayloadOperation> { PAYLOAD_OPERATIONS.with(|v| std::mem::take(&mut *v.borrow_mut())) }

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)] use std::os::unix::fs::symlink;
    fn fixture(name: &str) -> PathBuf { let p = std::env::temp_dir().join(format!("ccp-payload-{}-{name}", std::process::id())); let _ = fs::remove_dir_all(&p); fs::create_dir(&p).unwrap(); p }
    #[cfg(unix)]
    #[test] fn payload_measurement_counts_links_without_following_targets() { let p=fixture("links"); fs::write(p.join("regular"), b"abc").unwrap(); symlink("missing",p.join("broken")).unwrap(); symlink("regular",p.join("relative")).unwrap(); symlink("self",p.join("self")).unwrap(); let mut n=0; let s=measure_payload_tree(&p,&mut n,100).unwrap(); assert_eq!(s.files,4); assert_eq!(s.bytes,3+7+6+4); fs::remove_dir_all(p).unwrap(); }
    #[cfg(unix)]
    #[test] fn payload_root_itself_must_be_a_plain_directory() { let r=fixture("root"); let l=r.with_extension("link"); symlink(&r,&l).unwrap(); let mut n=0; assert!(matches!(measure_payload_tree(&l,&mut n,100),Err(CacheError::SymlinkInManagedRoot(_)))); fs::remove_file(l).unwrap(); fs::remove_dir(r).unwrap(); }
    #[cfg(unix)]
    #[test] fn unsupported_payload_object_fails_without_traversal() { use std::os::unix::net::UnixListener; let p=fixture("socket"); let s=p.join("s"); let _l=UnixListener::bind(&s).unwrap(); let mut n=0; assert!(matches!(measure_payload_tree(&p,&mut n,100),Err(CacheError::UnexpectedEntry(_)))); fs::remove_dir_all(p).unwrap(); }
    #[test] fn payload_node_limit_is_fail_closed() { let p=fixture("limit"); fs::write(p.join("a"),b"a").unwrap(); fs::write(p.join("b"),b"b").unwrap(); let mut n=0; assert!(matches!(measure_payload_tree(&p,&mut n,2),Err(CacheError::InventoryLimitExceeded))); fs::remove_dir_all(p).unwrap(); }
}
