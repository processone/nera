//! Private storage for the embedded runtime, independent of installed libraries.

use std::path::{Path, PathBuf};

/// Keep disposable extraction directories alive until the linker has finished.
pub(crate) struct RuntimeCache {
    pub(crate) path: PathBuf,
    base: PathBuf,
    _temporary: Option<tempfile::TempDir>,
}

impl RuntimeCache {
    pub(crate) fn new(base: &Path) -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            let home = std::env::var_os("HOME").map(PathBuf::from);
            Self::unix(base, home.as_deref(), effective_uid())
        }
        #[cfg(not(unix))]
        {
            // Windows uses the account's temporary directory; Unix also needs
            // ownership and ancestor checks for shared temporary roots.
            Ok(Self {
                path: base.to_path_buf(),
                base: base.to_path_buf(),
                _temporary: None,
            })
        }
    }

    #[cfg(unix)]
    fn unix(temp: &Path, home: Option<&Path>, uid: u32) -> std::io::Result<Self> {
        use std::os::unix::fs::DirBuilderExt;

        let (base, name) = match protected_base(temp, uid) {
            Ok(base) => (base, format!("nera-runtime-{uid}")),
            Err(temp_error) => {
                // HOME and its ancestors must protect cache paths from rename
                // swaps too. Unmapped Linux system owners are handled by the
                // home ancestry check without relaxing write/sticky checks.
                let home = home.ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!(
                            "TMPDIR {} cannot hold the runtime cache ({}); HOME is not set",
                            temp.display(),
                            temp_error
                        ),
                    )
                })?;
                let base = home_base(home, uid).map_err(|err| std::io::Error::new(
                    err.kind(),
                    format!("TMPDIR {} cannot hold the runtime cache ({}); HOME {} fallback failed: {}", temp.display(), temp_error, home.display(), err),
                ))?;
                (base, ".nera-runtime".to_owned())
            }
        };
        let path = base.join(name);
        // A name is only a lookup hint, never evidence of ownership. Do not
        // chmod or remove pre-existing entries: they may belong to a peer.
        let _ = std::fs::DirBuilder::new().mode(0o700).create(&path);
        if private_directory(&path, uid) {
            return Ok(Self {
                path,
                base,
                _temporary: None,
            });
        }
        Self::temporary_in(&base)
    }

    /// Use the already selected safe location, including the home fallback.
    /// Do not return to the original unsafe TMPDIR for an obstructed archive.
    pub(crate) fn temporary(&self) -> std::io::Result<Self> {
        Self::temporary_in(&self.base)
    }

    fn temporary_in(base: &Path) -> std::io::Result<Self> {
        let mut builder = tempfile::Builder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(std::fs::Permissions::from_mode(0o700));
        }
        let directory = builder.prefix("nera-runtime-").tempdir_in(base)?;
        Ok(Self {
            path: directory.path().to_path_buf(),
            base: base.to_path_buf(),
            _temporary: Some(directory),
        })
    }
}

#[cfg(unix)]
fn effective_uid() -> u32 {
    // SAFETY: geteuid takes no arguments and has no preconditions.
    unsafe { libc::geteuid() }
}

/// A private child is not safe if another account can rename its ancestors.
/// Canonicalize before validation so a mutable ancestor symlink is not retained
/// in the path passed to the linker. System-owned sticky /tmp is allowed.
#[cfg(unix)]
fn protected_base(base: &Path, uid: u32) -> std::io::Result<PathBuf> {
    let base = base.canonicalize()?;
    protected_ancestors(&base, uid, None, "TMPDIR")?;
    Ok(base)
}

#[cfg(unix)]
fn protected_ancestors(
    base: &Path,
    uid: u32,
    overflow: Option<u32>,
    label: &str,
) -> std::io::Result<()> {
    for ancestor in base.ancestors() {
        let metadata = std::fs::symlink_metadata(ancestor)?;
        if !protected_ancestor(&metadata, uid, overflow) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!(
                    "{} ancestor {} is not protected from other users",
                    label,
                    ancestor.display()
                ),
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn protected_ancestor(metadata: &std::fs::Metadata, uid: u32, overflow: Option<u32>) -> bool {
    use std::os::unix::fs::MetadataExt;

    metadata.is_dir()
        && (metadata.uid() == uid || metadata.uid() == 0 || Some(metadata.uid()) == overflow)
        && (metadata.mode() & 0o022 == 0 || metadata.mode() & 0o1000 != 0)
}

/// Only tolerate overflow ownership when Linux reports a non-initial UID map.
/// A real nobody-owned ancestor in the initial namespace is not trusted.
#[cfg(target_os = "linux")]
fn kernel_overflow_uid() -> Option<u32> {
    let mapping = std::fs::read_to_string("/proc/self/uid_map").ok()?;
    let fields: Vec<u64> = mapping
        .split_whitespace()
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    if fields.is_empty() || !fields.len().is_multiple_of(3) || fields == [0, 0, u64::from(u32::MAX)] {
        return None;
    }
    std::fs::read_to_string("/proc/sys/kernel/overflowuid")
        .ok()?
        .trim()
        .parse()
        .ok()
}

#[cfg(all(unix, not(target_os = "linux")))]
fn kernel_overflow_uid() -> Option<u32> {
    None
}

/// The configured account home must protect its cache entries from replacement.
#[cfg(unix)]
fn home_base(home: &Path, uid: u32) -> std::io::Result<PathBuf> {
    use std::os::unix::fs::MetadataExt;

    if !home.is_absolute() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "HOME must be an absolute account directory",
        ));
    }
    let home = home.canonicalize()?;
    let metadata = std::fs::symlink_metadata(&home)?;
    if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o022 != 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "HOME must be owned by the effective user and not writable by other users",
        ));
    }
    if let Some(parent) = home.parent() {
        protected_ancestors(parent, uid, kernel_overflow_uid(), "HOME")?;
    }
    Ok(home)
}

#[cfg(unix)]
fn private_directory(path: &Path, uid: u32) -> bool {
    use std::os::unix::fs::MetadataExt;

    std::fs::symlink_metadata(path).is_ok_and(|metadata| {
        metadata.is_dir() && metadata.uid() == uid && metadata.mode() & 0o777 == 0o700
    })
}

/// Classify the caller's one no-follow metadata reading; never stat again here.
pub(crate) fn reusable_archive(metadata: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        private_archive(metadata, effective_uid())
    }
    #[cfg(not(unix))]
    {
        metadata.is_file()
    }
}

#[cfg(unix)]
fn private_archive(metadata: &std::fs::Metadata, uid: u32) -> bool {
    use std::os::unix::fs::MetadataExt;

    metadata.is_file() && metadata.uid() == uid && metadata.mode() & 0o077 == 0
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, DirBuilderExt, PermissionsExt};

    // NERA-116 F1: fixture roots must also be protected under umask 002.
    fn private_tempdir() -> tempfile::TempDir {
        tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap()
    }

    /// NERA-61: writable ancestors must not let another account replace a private cache.
    #[test]
    fn cache_rejects_renameable_ancestors_but_accepts_sticky_shared_temp() {
        let base = private_tempdir();
        std::fs::set_permissions(base.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        let error = RuntimeCache::unix(base.path(), None, effective_uid())
            .err()
            .unwrap();
        assert!(error.to_string().contains("TMPDIR"));
        assert!(error
            .to_string()
            .contains(&base.path().canonicalize().unwrap().display().to_string()));
        std::fs::set_permissions(base.path(), std::fs::Permissions::from_mode(0o1777)).unwrap();
        assert!(RuntimeCache::unix(base.path(), None, effective_uid()).is_ok());
    }

    /// Captain's fallback-home choice: unsafe temp roots must not stop standalone builds.
    #[test]
    fn unsafe_temp_uses_private_reusable_home_cache() {
        let temp = private_tempdir();
        let home = private_tempdir();
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        let cache = RuntimeCache::unix(temp.path(), Some(home.path()), effective_uid()).unwrap();
        assert_eq!(
            cache.path.parent().unwrap(),
            home.path().canonicalize().unwrap()
        );
        assert!(private_directory(&cache.path, effective_uid()));
        let archive = tempfile::NamedTempFile::new_in(&cache.path).unwrap();
        assert!(private_archive(
            &std::fs::symlink_metadata(archive.path()).unwrap(),
            effective_uid()
        ));
        let reused = RuntimeCache::unix(temp.path(), Some(home.path()), effective_uid()).unwrap();
        assert_eq!(reused.path, cache.path);
        assert!(archive.path().is_file());
        let disposable = cache.temporary().unwrap();
        assert_eq!(
            disposable.path.parent().unwrap(),
            home.path().canonicalize().unwrap()
        );
        assert!(private_directory(&disposable.path, effective_uid()));
        let path = disposable.path.clone();
        drop(disposable);
        assert!(!path.exists());
    }

    /// NERA-116 N1: private HOME permissions cannot protect a replaceable ancestor.
    #[test]
    fn home_fallback_checks_every_ancestor_before_using_the_cache() {
        let root = private_tempdir();
        let temp = private_tempdir();
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        let shared = root.path().join("shared");
        std::fs::create_dir(&shared).unwrap();
        let home = shared.join("alice");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&home)
            .unwrap();
        for mode in [0o775, 0o777] {
            std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(mode)).unwrap();
            let error = RuntimeCache::unix(temp.path(), Some(&home), effective_uid())
                .err()
                .unwrap();
            assert!(error.to_string().contains("HOME ancestor"));
            assert!(error
                .to_string()
                .contains(&shared.canonicalize().unwrap().display().to_string()));
            assert!(!home.join(".nera-runtime").exists());
        }
        for mode in [0o755, 0o1777] {
            std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(mode)).unwrap();
            let cache = RuntimeCache::unix(temp.path(), Some(&home), effective_uid()).unwrap();
            assert_eq!(cache.path.parent().unwrap(), home.canonicalize().unwrap());
            assert!(private_directory(&cache.path, effective_uid()));
        }
    }

    /// NERA-116 N1: overflow ownership never bypasses the write/sticky requirement.
    #[test]
    fn ancestor_ownership_allows_only_user_root_or_explicit_overflow() {
        use std::os::unix::fs::MetadataExt;

        let directory = private_tempdir();
        let metadata = std::fs::symlink_metadata(directory.path()).unwrap();
        let owner = metadata.uid();
        assert!(protected_ancestor(&metadata, owner, None));
        if owner != 0 {
            let foreign_user = owner.wrapping_add(1);
            assert!(!protected_ancestor(&metadata, foreign_user, None));
            assert!(protected_ancestor(&metadata, foreign_user, Some(owner)));
        }
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        let metadata = std::fs::symlink_metadata(directory.path()).unwrap();
        assert!(!protected_ancestor(&metadata, owner, Some(owner)));
    }

    /// Captain's fallback-home choice does not authorize another account's home or cache.
    #[test]
    fn home_fallback_requires_owned_protected_account_root() {
        let home = private_tempdir();
        assert!(home_base(home.path(), effective_uid()).is_ok());
        assert!(home_base(home.path(), effective_uid().wrapping_add(1)).is_err());
        assert!(home_base(Path::new("relative-home"), effective_uid()).is_err());
        let file = tempfile::NamedTempFile::new_in(home.path()).unwrap();
        assert!(home_base(file.path(), effective_uid()).is_err());
        std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(home_base(home.path(), effective_uid()).is_err());
    }

    /// NERA-61: cache ownership, not just its public name, is a trust boundary.
    #[test]
    fn cache_requires_private_directory_owned_by_effective_user() {
        let base = private_tempdir();
        let cache = RuntimeCache::new(base.path()).unwrap();
        assert!(private_directory(&cache.path, effective_uid()));
        assert!(!private_directory(
            &cache.path,
            effective_uid().wrapping_add(1)
        ));
        assert_eq!(RuntimeCache::new(base.path()).unwrap().path, cache.path);
    }

    /// NERA-61 and fallback-home: preclaimed cache names cannot deny extraction or be repaired.
    #[test]
    fn obstructed_cache_uses_disposable_private_directory() {
        for use_home in [false, true] {
            for kind in ["file", "directory", "symlink", "dangling"] {
                let base = private_tempdir();
                let home = private_tempdir();
                let safe_base = if use_home {
                    std::fs::set_permissions(base.path(), std::fs::Permissions::from_mode(0o777))
                        .unwrap();
                    home.path()
                } else {
                    base.path()
                };
                let path = safe_base.join(if use_home {
                    ".nera-runtime".to_owned()
                } else {
                    format!("nera-runtime-{}", effective_uid())
                });
                let target = private_tempdir();
                match kind {
                    "file" => std::fs::write(&path, b"preclaimed cache").unwrap(),
                    "directory" => {
                        std::fs::create_dir(&path).unwrap();
                        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o777))
                            .unwrap();
                    }
                    "symlink" => symlink(target.path(), &path).unwrap(),
                    _ => symlink(target.path().join("absent"), &path).unwrap(),
                }
                let cache =
                    RuntimeCache::unix(base.path(), Some(home.path()), effective_uid()).unwrap();
                assert_ne!(cache.path, path, "{kind}");
                assert_eq!(
                    cache.path.parent().unwrap(),
                    safe_base.canonicalize().unwrap()
                );
                assert!(private_directory(&cache.path, effective_uid()), "{kind}");
                assert!(std::fs::symlink_metadata(&path).is_ok(), "removed {kind}");
                let disposable = cache.path.clone();
                drop(cache);
                assert!(!disposable.exists(), "leaked {kind} fallback");
            }
        }
    }

    /// NERA-61: neither initial reuse nor failed-publication reuse may follow an unsafe archive.
    #[test]
    fn archive_requires_regular_private_file_owned_by_effective_user() {
        let directory = private_tempdir();
        let file = tempfile::NamedTempFile::new_in(directory.path()).unwrap();
        let metadata = std::fs::symlink_metadata(file.path()).unwrap();
        assert!(private_archive(&metadata, effective_uid()));
        assert!(!private_archive(&metadata, effective_uid().wrapping_add(1)));
        assert!(!reusable_archive(
            &std::fs::symlink_metadata(directory.path()).unwrap()
        ));
        let link = directory.path().join("runtime.a");
        symlink(file.path(), &link).unwrap();
        assert!(link.is_file());
        assert!(!reusable_archive(
            &std::fs::symlink_metadata(&link).unwrap()
        ));
        std::fs::set_permissions(file.path(), std::fs::Permissions::from_mode(0o666)).unwrap();
        assert!(!reusable_archive(
            &std::fs::symlink_metadata(file.path()).unwrap()
        ));
    }
}
