use crate::{CoreError, Result};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn private_dir(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if !meta.is_dir() || meta.file_type().is_symlink() => {
            return Err(CoreError::Storage);
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(path)?;
        }
        Err(_) => return Err(CoreError::Storage),
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

pub(crate) fn regular_file(path: &Path) -> Result<File> {
    let meta = fs::symlink_metadata(path).map_err(|_| CoreError::MissingContent)?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.nlink() != 1 {
        return Err(CoreError::MissingContent);
    }
    let file = File::open(path).map_err(|_| CoreError::MissingContent)?;
    let opened = file.metadata().map_err(|_| CoreError::MissingContent)?;
    if meta.ino() != opened.ino() || meta.dev() != opened.dev() {
        return Err(CoreError::MissingContent);
    }
    Ok(file)
}

pub(crate) struct BlobStore {
    dir: PathBuf,
    identity: (u64, u64),
}

impl BlobStore {
    pub(crate) fn open(root: &Path) -> Result<Self> {
        let dir = root.join("blobs");
        private_dir(&dir)?;
        let meta = fs::symlink_metadata(&dir)?;
        Ok(Self {
            dir,
            identity: (meta.dev(), meta.ino()),
        })
    }

    fn check_dir(&self) -> Result<()> {
        let meta = fs::symlink_metadata(&self.dir)?;
        if !meta.is_dir()
            || meta.file_type().is_symlink()
            || (meta.dev(), meta.ino()) != self.identity
        {
            return Err(CoreError::Storage);
        }
        Ok(())
    }

    fn path(&self, digest: &str) -> Result<PathBuf> {
        self.check_dir()?;
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(CoreError::MissingContent);
        }
        Ok(self.dir.join(format!("{digest}.blob")))
    }

    pub(crate) fn read(&self, digest: &str, size: usize) -> Result<Vec<u8>> {
        if size > crate::models::MAX_PAYLOAD_BYTES {
            return Err(CoreError::MissingContent);
        }
        let file = regular_file(&self.path(digest)?)?;
        if file
            .metadata()
            .map_err(|_| CoreError::MissingContent)?
            .len()
            != size as u64
        {
            return Err(CoreError::MissingContent);
        }
        let mut bytes = Vec::with_capacity(size);
        file.take(size as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| CoreError::MissingContent)?;
        if bytes.len() != size || blake3::hash(&bytes).to_hex().as_str() != digest {
            return Err(CoreError::MissingContent);
        }
        Ok(bytes)
    }

    // Callers hold the database writer lock through staging and reference commit.
    pub(crate) fn put(&self, bytes: &[u8]) -> Result<String> {
        let digest = blake3::hash(bytes).to_hex().to_string();
        let target = self.path(&digest)?;
        if fs::symlink_metadata(&target).is_ok() {
            self.read(&digest, bytes.len())?;
            return Ok(digest);
        }
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temporary = self.dir.join(format!(
            "stage-{}-{stamp}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, &target)?;
        File::open(&self.dir)?.sync_all()?;
        Ok(digest)
    }

    pub(crate) fn remove(&self, digest: &str) -> Result<bool> {
        let path = self.path(digest)?;
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(_) => Err(CoreError::Storage),
            Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() && meta.nlink() == 1 => {
                fs::remove_file(path)?;
                Ok(true)
            }
            Ok(_) => Err(CoreError::Storage),
        }
    }

    pub(crate) fn reconcile(&self, live: &std::collections::HashSet<String>) -> Result<usize> {
        self.check_dir()?;
        let mut removed = 0;
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let meta = fs::symlink_metadata(entry.path())?;
            if !meta.is_file() || meta.file_type().is_symlink() || meta.nlink() != 1 {
                continue;
            }
            let digest = name.strip_suffix(".blob");
            let derived = digest.is_some_and(|d| self.path(d).is_ok() && !live.contains(d));
            let staged = name.starts_with("stage-");
            let old = meta
                .modified()
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age >= Duration::from_secs(3600));
            // A grace period also protects abandoned writes from a recently exited owner.
            if (derived || staged) && old {
                fs::remove_file(entry.path())?;
                removed += 1;
            }
        }
        Ok(removed)
    }
}
