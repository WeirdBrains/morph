//! Where the AI files go: one seam for every file the companions keep (the JSON files of a
//! [`FileStore`](crate::FileStore), the MCP approvals and secrets). The default writes plain files;
//! the desktop plugs in the profile's sealed files, so this crate needs no key and no crypto.

use std::path::Path;

use crate::store::StoreError;

/// Reads and writes whole files. `read` is `Ok(None)` for a file that does not exist and
/// [`StoreError::Locked`] for one that exists but cannot be opened yet (the profile is locked);
/// `write` is atomic and never partly overwrites what it could not seal.
pub trait FileIo: Send + Sync {
    fn read(&self, path: &Path) -> Result<Option<Vec<u8>>, StoreError>;
    fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), StoreError>;
}

/// Plain files (readable by the user only), for tests and profiles with no identity.
#[derive(Debug, Default, Clone, Copy)]
pub struct PlainIo;

impl FileIo for PlainIo {
    fn read(&self, path: &Path) -> Result<Option<Vec<u8>>, StoreError> {
        match std::fs::read(path) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(StoreError::Io(e.to_string())),
        }
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
        crate::write_private(path, bytes).map_err(|e| StoreError::Io(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_round_trip_and_missing() {
        let dir = std::env::temp_dir().join(format!("morph-fileio-{}", std::process::id()));
        let path = dir.join("a/b.json");
        assert!(PlainIo.read(&path).unwrap().is_none());
        PlainIo.write(&path, b"hi").unwrap();
        assert_eq!(PlainIo.read(&path).unwrap().as_deref(), Some(&b"hi"[..]));
        let _ = std::fs::remove_dir_all(dir);
    }
}
