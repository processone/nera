//! Application registry for `-include_lib` resolution.
//!
//! Maps OTP application names (e.g. `kernel`, `cowboy`) to their root
//! directories on disk, enabling resolution of `-include_lib("app/path")`
//! directives without a running BEAM VM.
//!
//! The registry is populated from multiple sources in priority order:
//! 1. OTP standard library apps (lowest priority)
//! 2. Project dependencies (`--deps-dir`)
//! 3. Project's own source directories (highest priority)

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::otp;

/// Thread-safe callback resolving Erlang include paths.
pub type IncludeLibResolver = dyn Fn(&str) -> Option<PathBuf> + Send + Sync;

/// Registry mapping application names to their root directories.
#[derive(Debug, Clone)]
pub struct AppRegistry {
    /// app name → app root directory
    apps: HashMap<String, PathBuf>,
    otp_release: Option<u32>,
}

impl AppRegistry {
    /// Build a registry from the available sources.
    ///
    /// Sources are applied in order so later ones override earlier:
    /// 1. OTP stdlib apps (from `otp_root`)
    /// 2. Dependency directories (from `deps_dirs`)
    /// 3. Project source directories (from `src_dirs`)
    pub fn build(
        otp_root: Option<&Path>,
        deps_dirs: &[PathBuf],
        src_dirs: &[PathBuf],
    ) -> Self {
        let mut apps = HashMap::new();

        // 1. OTP standard library apps (lowest priority)
        if let Some(root) = otp_root {
            let otp_apps = otp::scan_otp_apps(root);
            apps.extend(otp_apps);
        }

        // 2. Dependencies (each subdirectory of deps_dir is an app)
        for deps_dir in deps_dirs {
            if !deps_dir.is_dir() {
                continue;
            }
            let entries = match std::fs::read_dir(deps_dir) {
                Ok(e) => e,
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    apps.insert(name.to_string(), path);
                }
            }
        }

        // 3. Project source directories (highest priority)
        // Each src_dir's parent might be an app root (e.g. apps/myapp/src → myapp)
        for src_dir in src_dirs {
            if let Some(parent) = src_dir.parent() {
                if let Some(name) = parent.file_name().and_then(|n| n.to_str()) {
                    // Only treat it as an app if the parent has the conventional
                    // OTP app structure (contains src/ or include/).
                    let has_src = parent.join("src").is_dir();
                    let has_include = parent.join("include").is_dir();
                    if has_src || has_include {
                        apps.insert(name.to_string(), parent.to_path_buf());
                    }
                }
            }
        }

        Self { apps, otp_release: otp_root.and_then(otp::otp_release) }
    }

    /// Release associated with this registry's selected OTP installation.
    pub fn otp_release(&self) -> Option<u32> { self.otp_release }

    /// Resolve an `-include_lib` path to an absolute file path.
    ///
    /// The input is the string inside the directive, e.g. `"kernel/include/file.hrl"`.
    /// The first path component is treated as the application name, and the
    /// remainder is appended to the app's root directory.
    ///
    /// Returns `Ok(path)` on success, or `Err(IncludeLibError)` with diagnostic
    /// information about what was searched.
    pub fn resolve_include_lib(&self, include_path: &str) -> Result<PathBuf, IncludeLibError> {
        // Split on the first '/'
        let (app_name, remainder) = match include_path.find('/') {
            Some(pos) => (&include_path[..pos], &include_path[pos + 1..]),
            None => {
                return Err(IncludeLibError::InvalidPath {
                    path: include_path.to_string(),
                });
            }
        };

        if app_name.is_empty() || remainder.is_empty() {
            return Err(IncludeLibError::InvalidPath {
                path: include_path.to_string(),
            });
        }

        match self.apps.get(app_name) {
            Some(app_dir) => {
                let full_path = app_dir.join(remainder);
                if full_path.is_file() {
                    Ok(full_path)
                } else {
                    Err(IncludeLibError::FileNotFound {
                        app_name: app_name.to_string(),
                        app_dir: app_dir.clone(),
                        file_path: full_path,
                    })
                }
            }
            None => Err(IncludeLibError::AppNotFound {
                app_name: app_name.to_string(),
                searched: self.searched_locations(),
            }),
        }
    }

    /// Create a resolver closure suitable for passing to the preprocessor.
    ///
    /// The returned closure resolves `-include_lib` paths using this registry.
    pub fn make_resolver(&self) -> Box<IncludeLibResolver> {
        let registry = self.clone();
        Box::new(move |path: &str| registry.resolve_include_lib(path).ok())
    }

    /// Return a summary of where apps were searched (for diagnostics).
    fn searched_locations(&self) -> Vec<String> {
        let mut locations: Vec<String> = self
            .apps
            .values()
            .filter_map(|p| p.parent())
            .map(|p| p.display().to_string())
            .collect();
        locations.sort();
        locations.dedup();
        locations
    }

    /// Return the number of registered applications.
    pub fn len(&self) -> usize {
        self.apps.len()
    }

    /// Check if the registry is empty.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.apps.is_empty()
    }
}

/// Error type for `-include_lib` resolution failures.
#[derive(Debug)]
pub enum IncludeLibError {
    /// The include path doesn't have the expected `"app/rest"` format.
    InvalidPath { path: String },
    /// The application name was not found in any searched location.
    AppNotFound {
        app_name: String,
        searched: Vec<String>,
    },
    /// The application was found but the specific file doesn't exist within it.
    FileNotFound {
        app_name: String,
        app_dir: PathBuf,
        file_path: PathBuf,
    },
}

impl std::fmt::Display for IncludeLibError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPath { path } => {
                write!(
                    f,
                    "invalid -include_lib path \"{path}\": expected \"app/path/to/file.hrl\""
                )
            }
            Self::AppNotFound {
                app_name,
                searched,
            } => {
                write!(
                    f,
                    "application \"{app_name}\" not found for -include_lib"
                )?;
                if !searched.is_empty() {
                    write!(f, "\n  searched in:")?;
                    for loc in searched {
                        write!(f, "\n    - {loc}")?;
                    }
                }
                write!(
                    f,
                    "\n  hint: set [otp] root in nera.toml or NERA_OTP_ROOT env var"
                )
            }
            Self::FileNotFound {
                app_name,
                app_dir,
                file_path,
            } => {
                write!(
                    f,
                    "file not found for -include_lib: {} (app \"{app_name}\" resolved to {})",
                    file_path.display(),
                    app_dir.display()
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_resolve_include_lib() {
        let tmp = tempfile::tempdir().unwrap();
        let app_dir = tmp.path().join("myapp");
        let include_dir = app_dir.join("include");
        fs::create_dir_all(&include_dir).unwrap();
        fs::write(include_dir.join("myapp.hrl"), "%% header\n").unwrap();

        let mut apps = HashMap::new();
        apps.insert("myapp".to_string(), app_dir.clone());

        let registry = AppRegistry { apps, otp_release: None };

        // Successful resolution
        let result = registry.resolve_include_lib("myapp/include/myapp.hrl");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), include_dir.join("myapp.hrl"));

        // App not found
        let result = registry.resolve_include_lib("unknown/include/foo.hrl");
        assert!(matches!(result, Err(IncludeLibError::AppNotFound { .. })));

        // File not found within app
        let result = registry.resolve_include_lib("myapp/include/missing.hrl");
        assert!(matches!(result, Err(IncludeLibError::FileNotFound { .. })));

        // Invalid path
        let result = registry.resolve_include_lib("noslash");
        assert!(matches!(result, Err(IncludeLibError::InvalidPath { .. })));
    }
}
