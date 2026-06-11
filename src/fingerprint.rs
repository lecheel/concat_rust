use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use crate::stable_hash::stable_hash_file;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FileFingerprint {
    /// SHA-256 of file contents (first 24 hex chars)
    pub content_hash: String,
    /// Nanosecond mtime for quick rejection
    pub mtime_ns: u128,
    /// File size in bytes for quick rejection
    pub size: u64,
}

impl FileFingerprint {
    /// Compute a fingerprint for a file on disk.
    /// Reads the raw bytes to generate the content hash.
    pub fn compute(path: &Path) -> std::io::Result<Self> {
        let meta = std::fs::metadata(path)?;
        let size = meta.len();

        let mtime_ns = meta
            .modified()?
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();

        // Read raw bytes for hashing
        let content = std::fs::read(path)?;
        let content_hash = stable_hash_file(&content);

        Ok(Self {
            content_hash,
            mtime_ns,
            size,
        })
    }

    /// Determine if the file has changed compared to a previous fingerprint.
    /// Uses a two-tier strategy:
    /// 1. Fast path: if mtime AND size are identical, assume unchanged.
    /// 2. Slow path: if mtime or size differ, check content hash.
    pub fn is_dirty(&self, current: &Self) -> bool {
        // Fast path: if metadata matches exactly, content cannot have changed
        if self.mtime_ns == current.mtime_ns && self.size == current.size {
            return false;
        }

        // Metadata changed, but content might be the same (e.g., touched file)
        // Definitive check via hash
        self.content_hash != current.content_hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn make_file(content: &[u8]) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content).unwrap();
        file.flush().unwrap();
        file
    }

    #[test]
    fn same_file_not_dirty() {
        let file = make_file(b"fn main() {}");
        let fp1 = FileFingerprint::compute(file.path()).unwrap();
        let fp2 = FileFingerprint::compute(file.path()).unwrap();
        assert!(!fp1.is_dirty(&fp2), "Identical file should not be dirty");
    }

    #[test]
    fn changed_content_is_dirty() {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "version 1").unwrap();
        file.flush().unwrap();
        let fp1 = FileFingerprint::compute(file.path()).unwrap();

        // Write new content
        write!(file, "version 2 is longer").unwrap();
        file.flush().unwrap();

        let fp2 = FileFingerprint::compute(file.path()).unwrap();
        assert!(fp1.is_dirty(&fp2), "Changed content must be dirty");
    }

    #[test]
    fn metadata_change_same_content_not_dirty() {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "stable content").unwrap();
        file.flush().unwrap();
        let fp1 = FileFingerprint::compute(file.path()).unwrap();

        // Force a metadata change (touch) without changing content
        let now = SystemTime::now();
        file.as_file_mut().set_modified(now).unwrap();

        let fp2 = FileFingerprint::compute(file.path()).unwrap();

        // mtime differs, but hash is the same
        assert_ne!(fp1.mtime_ns, fp2.mtime_ns, "Mtime should differ");
        assert_eq!(fp1.content_hash, fp2.content_hash, "Hash should be same");
        assert!(
            !fp1.is_dirty(&fp2),
            "Touched file with same content should not be dirty"
        );
    }
}
