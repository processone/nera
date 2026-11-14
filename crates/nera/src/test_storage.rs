//! Fresh private storage for each native test executable and its object files.

use std::path::Path;

pub(crate) fn new() -> std::io::Result<tempfile::TempDir> {
    #[cfg(unix)]
    {
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        // SAFETY: geteuid takes no arguments and has no preconditions.
        unix(&std::env::temp_dir(), home.as_deref(), unsafe {
            libc::geteuid()
        })
    }
    #[cfg(not(unix))]
    {
        // Windows uses the account's temporary directory and inherited ACLs.
        create(&std::env::temp_dir())
    }
}

#[cfg(unix)]
fn unix(temp: &Path, home: Option<&Path>, uid: u32) -> std::io::Result<tempfile::TempDir> {
    let base = crate::runtime_cache::protected_base(temp, uid).or_else(|temp_error| {
        let home = home.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "TMPDIR {} is not protected ({}); HOME is not set",
                    temp.display(),
                    temp_error
                ),
            )
        })?;
        crate::runtime_cache::home_base(home, uid).map_err(|err| {
            std::io::Error::new(
                err.kind(),
                format!(
                    "TMPDIR {} is not protected ({}); HOME {} fallback failed: {}",
                    temp.display(),
                    temp_error,
                    home.display(),
                    err
                ),
            )
        })
    })?;
    create(&base)
}

fn create(base: &Path) -> std::io::Result<tempfile::TempDir> {
    let mut builder = tempfile::Builder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Apply at mkdir, not afterwards: even umask 000 must expose no window.
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    // tempfile creates the randomized name exclusively; existing entries are
    // never reused, repaired or removed. Retain its guard through execution.
    builder.prefix("nera-test-").tempdir_in(base)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};

    fn fixture() -> tempfile::TempDir {
        create(&std::env::temp_dir()).unwrap()
    }

    fn uid() -> u32 {
        // SAFETY: geteuid takes no arguments and has no preconditions.
        unsafe { libc::geteuid() }
    }

    #[test]
    fn directories_are_fresh_private_owned_and_removed_on_drop() {
        let base = fixture();
        let old = base
            .path()
            .join(format!("nera_tests_{}", std::process::id()));
        let target = fixture();
        let sentinel = target.path().join("sentinel");
        std::fs::write(&sentinel, b"untouched").unwrap();
        symlink(target.path(), &old).unwrap();
        let first = unix(base.path(), None, uid()).unwrap();
        let second = unix(base.path(), None, uid()).unwrap();
        assert_ne!(first.path(), second.path());
        for directory in [&first, &second] {
            let metadata = std::fs::symlink_metadata(directory.path()).unwrap();
            assert!(metadata.is_dir());
            assert_eq!(metadata.uid(), uid());
            assert_eq!(metadata.mode() & 0o777, 0o700);
            assert_eq!(
                directory.path().parent().unwrap(),
                base.path().canonicalize().unwrap()
            );
        }
        let path = first.path().to_owned();
        std::fs::write(path.join("test.o"), b"object").unwrap();
        drop(first);
        assert!(!path.exists());
        assert!(std::fs::symlink_metadata(&old).unwrap().is_symlink());
        assert_eq!(std::fs::read(sentinel).unwrap(), b"untouched");
    }

    #[test]
    fn unsafe_temp_uses_home_but_unsafe_home_fails_closed() {
        let base = fixture();
        let home = fixture();
        std::fs::set_permissions(base.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        let directory = unix(base.path(), Some(home.path()), uid()).unwrap();
        assert_eq!(
            directory.path().parent().unwrap(),
            home.path().canonicalize().unwrap()
        );
        assert_eq!(
            std::fs::metadata(directory.path()).unwrap().mode() & 0o777,
            0o700
        );
        assert!(unix(base.path(), None, uid())
            .unwrap_err()
            .to_string()
            .contains("HOME"));
        assert!(unix(base.path(), Some(home.path()), uid().wrapping_add(1)).is_err());
        std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(unix(base.path(), Some(home.path()), uid()).is_err());
    }

    #[test]
    fn writable_ancestors_are_rejected_and_sticky_roots_are_supported() {
        let base = fixture();
        let nested = base.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        std::fs::set_permissions(base.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(unix(&nested, None, uid()).is_err());
        std::fs::set_permissions(base.path(), std::fs::Permissions::from_mode(0o1777)).unwrap();
        assert!(unix(&nested, None, uid()).is_ok());
        assert!(unix(base.path(), None, uid()).is_ok());
    }

    #[test]
    fn canonical_path_does_not_retain_replaceable_symlink_alias() {
        let base = fixture();
        let alias = base.path().join("alias");
        let target = fixture();
        symlink(target.path(), &alias).unwrap();
        let directory = unix(&alias, None, uid()).unwrap();
        std::fs::remove_file(alias).unwrap();
        assert!(directory.path().is_dir());
        assert_eq!(
            directory.path().parent().unwrap(),
            target.path().canonicalize().unwrap()
        );
    }
}
