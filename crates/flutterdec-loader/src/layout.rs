//! Where the packaged data and the writable adapter store live.
//!
//! Discovery is deterministic and never depends on the current directory. A
//! released binary finds its own read-only data next to itself, and its
//! writable state under the user's data home. Nothing walks upward looking for
//! `Cargo.toml` or a repository manifest, because a decompiler that behaves
//! differently depending on where it was invoked from is a decompiler whose
//! results cannot be reproduced.
//!
//! Read-only package data (compatibility registry, runtime profiles, the
//! checked-in producer) is resolved in this order, first hit wins:
//!
//! 1. `FLUTTERDEC_DATA_DIR`, when set. An explicit override never silently
//!    falls back: if it does not hold a registry, resolution fails.
//! 2. `<exe dir>/../share/flutterdec`, the installed prefix layout.
//! 3. `<exe dir>`, a flat unpacked distribution.
//! 4. `<exe dir>/../..`, which is where a `cargo build` binary sits inside a
//!    checkout. This is a fixed position relative to the executable, not a
//!    search: exactly one directory is examined.
//!
//! The writable adapter store is `FLUTTERDEC_ADAPTER_STORE` when set, else
//! `<data home>/flutterdec/adapters`, where the data home is `XDG_DATA_HOME`
//! or `$HOME/.local/share`. The local symbol cache follows the same rule under
//! `FLUTTERDEC_SYMBOL_CACHE` and `<data home>/flutterdec/symbols`.

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

/// Environment variable naming the read-only package data directory.
pub const DATA_DIR_VAR: &str = "FLUTTERDEC_DATA_DIR";
/// Environment variable naming the writable adapter store directory.
pub const STORE_DIR_VAR: &str = "FLUTTERDEC_ADAPTER_STORE";
/// Environment variable naming the writable local symbol cache directory.
pub const SYMBOL_CACHE_VAR: &str = "FLUTTERDEC_SYMBOL_CACHE";

/// Package-data path, relative to the data directory.
pub const REGISTRY_RELATIVE_PATH: &str = "adapters/registry.json";
/// Checked-in reference producer, relative to the data directory.
pub const PRODUCER_RELATIVE_PATH: &str = "adapters/python/adapter_template.py";

/// Which discovery rule produced the data directory.
///
/// Reported so an operator can tell a packaged run from a build-tree run
/// without guessing from the path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataSource {
    /// `FLUTTERDEC_DATA_DIR`.
    Override,
    /// `<exe dir>/../share/flutterdec`.
    PackagePrefix,
    /// `<exe dir>`.
    ExecutableDirectory,
    /// `<exe dir>/../..`.
    BuildTree,
    /// Constructed directly, for tests and library callers.
    Explicit,
}

impl DataSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Override => "override",
            Self::PackagePrefix => "package_prefix",
            Self::ExecutableDirectory => "executable_directory",
            Self::BuildTree => "build_tree",
            Self::Explicit => "explicit",
        }
    }
}

impl fmt::Display for DataSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutError {
    /// The current executable could not be located.
    Executable(String),
    /// An explicit override was set but does not hold package data.
    Override { var: String, path: PathBuf },
    /// No candidate held package data.
    NoDataDirectory { candidates: Vec<PathBuf> },
    /// Neither `XDG_DATA_HOME` nor `HOME` is usable and no override was given.
    NoDataHome { var: String },
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Executable(detail) => {
                write!(f, "cannot locate the running executable: {detail}")
            }
            Self::Override { var, path } => write!(
                f,
                "{var} is set to {} but {} is missing",
                path.display(),
                path.join(REGISTRY_RELATIVE_PATH).display()
            ),
            Self::NoDataDirectory { candidates } => {
                write!(
                    f,
                    "no packaged data directory holds {REGISTRY_RELATIVE_PATH}; looked at"
                )?;
                for candidate in candidates {
                    write!(f, " {}", candidate.display())?;
                }
                write!(f, ". Set {DATA_DIR_VAR} to the directory that does")
            }
            Self::NoDataHome { var } => write!(
                f,
                "cannot place the writable adapter store: neither XDG_DATA_HOME nor HOME is set. Set {var}"
            ),
        }
    }
}

impl std::error::Error for LayoutError {}

/// The resolved locations one process works with.
///
/// Resolved once per run and passed down, so every command in one invocation
/// agrees about where the registry, the profiles, and the store are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    data_dir: PathBuf,
    store_dir: PathBuf,
    symbols_dir: PathBuf,
    data_source: DataSource,
}

fn env_path(get: &dyn Fn(&str) -> Option<OsString>, var: &str) -> Option<PathBuf> {
    get(var)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn holds_package_data(dir: &Path) -> bool {
    dir.join(REGISTRY_RELATIVE_PATH).is_file()
}

impl Layout {
    /// Resolve from the real process environment and executable path.
    pub fn resolve() -> Result<Self, LayoutError> {
        let exe = std::env::current_exe().map_err(|err| LayoutError::Executable(err.to_string()))?;
        Self::resolve_with(&exe, &|var| std::env::var_os(var))
    }

    /// Resolve against a supplied executable path and environment reader.
    ///
    /// Separated from [`Layout::resolve`] so discovery can be tested without
    /// mutating process-global environment state, which is what makes
    /// environment-dependent tests flake when run in parallel.
    pub fn resolve_with(
        exe: &Path,
        get: &dyn Fn(&str) -> Option<OsString>,
    ) -> Result<Self, LayoutError> {
        let (data_dir, data_source) = Self::resolve_data_dir(exe, get)?;

        let data_home = env_path(get, "XDG_DATA_HOME")
            .or_else(|| env_path(get, "HOME").map(|home| home.join(".local/share")));

        let store_dir = match env_path(get, STORE_DIR_VAR) {
            Some(path) => path,
            None => data_home
                .as_ref()
                .ok_or_else(|| LayoutError::NoDataHome {
                    var: STORE_DIR_VAR.to_string(),
                })?
                .join("flutterdec/adapters"),
        };
        let symbols_dir = match env_path(get, SYMBOL_CACHE_VAR) {
            Some(path) => path,
            None => data_home
                .as_ref()
                .ok_or_else(|| LayoutError::NoDataHome {
                    var: SYMBOL_CACHE_VAR.to_string(),
                })?
                .join("flutterdec/symbols"),
        };

        Ok(Self {
            data_dir,
            store_dir,
            symbols_dir,
            data_source,
        })
    }

    fn resolve_data_dir(
        exe: &Path,
        get: &dyn Fn(&str) -> Option<OsString>,
    ) -> Result<(PathBuf, DataSource), LayoutError> {
        if let Some(path) = env_path(get, DATA_DIR_VAR) {
            if !holds_package_data(&path) {
                return Err(LayoutError::Override {
                    var: DATA_DIR_VAR.to_string(),
                    path,
                });
            }
            return Ok((path, DataSource::Override));
        }

        let exe_dir = exe.parent().unwrap_or(Path::new("."));
        let candidates = [
            (exe_dir.join("../share/flutterdec"), DataSource::PackagePrefix),
            (exe_dir.to_path_buf(), DataSource::ExecutableDirectory),
            (exe_dir.join("../.."), DataSource::BuildTree),
        ];
        for (candidate, source) in &candidates {
            if holds_package_data(candidate) {
                let resolved = candidate.canonicalize().unwrap_or_else(|_| candidate.clone());
                return Ok((resolved, *source));
            }
        }
        Err(LayoutError::NoDataDirectory {
            candidates: candidates
                .into_iter()
                .map(|(candidate, _)| candidate)
                .collect(),
        })
    }

    /// Construct a layout directly. Used by tests and embedding callers.
    pub fn new(data_dir: PathBuf, store_dir: PathBuf, symbols_dir: PathBuf) -> Self {
        Self {
            data_dir,
            store_dir,
            symbols_dir,
            data_source: DataSource::Explicit,
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn store_dir(&self) -> &Path {
        &self.store_dir
    }

    pub fn symbols_dir(&self) -> &Path {
        &self.symbols_dir
    }

    pub fn data_source(&self) -> DataSource {
        self.data_source
    }

    pub fn registry_path(&self) -> PathBuf {
        self.data_dir.join(REGISTRY_RELATIVE_PATH)
    }

    /// The checked-in reference producer, which `adapter install` publishes
    /// when no artifact source is given.
    pub fn producer_path(&self) -> PathBuf {
        self.data_dir.join(PRODUCER_RELATIVE_PATH)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;
    use tempfile::tempdir;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map = pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), OsString::from(*value)))
            .collect::<HashMap<_, _>>();
        move |var: &str| map.get(var).cloned()
    }

    fn seed_data(dir: &Path) {
        fs::create_dir_all(dir.join("adapters")).expect("mkdir adapters");
        fs::write(dir.join(REGISTRY_RELATIVE_PATH), "{}").expect("write registry");
    }

    #[test]
    fn an_override_wins_and_never_falls_back() {
        let td = tempdir().expect("tempdir");
        let data = td.path().join("packaged");
        seed_data(&data);
        let exe = td.path().join("bin/flutterdec");

        let layout = Layout::resolve_with(
            &exe,
            &env(&[
                (DATA_DIR_VAR, data.to_str().unwrap()),
                ("HOME", td.path().to_str().unwrap()),
            ]),
        )
        .expect("resolve");
        assert_eq!(layout.data_dir(), data);
        assert_eq!(layout.data_source(), DataSource::Override);

        let empty = td.path().join("empty");
        fs::create_dir_all(&empty).expect("mkdir empty");
        let err = Layout::resolve_with(
            &exe,
            &env(&[
                (DATA_DIR_VAR, empty.to_str().unwrap()),
                ("HOME", td.path().to_str().unwrap()),
            ]),
        )
        .expect_err("an override that holds no registry is an error");
        assert!(matches!(err, LayoutError::Override { .. }), "{err}");
    }

    #[test]
    fn a_packaged_prefix_is_found_next_to_the_executable() {
        let td = tempdir().expect("tempdir");
        let prefix = td.path().join("prefix");
        fs::create_dir_all(prefix.join("bin")).expect("mkdir bin");
        seed_data(&prefix.join("share/flutterdec"));
        let exe = prefix.join("bin/flutterdec");

        let layout = Layout::resolve_with(&exe, &env(&[("HOME", td.path().to_str().unwrap())]))
            .expect("resolve");
        assert_eq!(layout.data_source(), DataSource::PackagePrefix);
        assert!(layout.registry_path().is_file());
        assert_eq!(
            layout.store_dir(),
            td.path().join(".local/share/flutterdec/adapters")
        );
        assert_eq!(
            layout.symbols_dir(),
            td.path().join(".local/share/flutterdec/symbols")
        );
    }

    #[test]
    fn a_build_tree_binary_finds_the_checkout_it_was_built_in() {
        let td = tempdir().expect("tempdir");
        let root = td.path().join("checkout");
        fs::create_dir_all(root.join("target/release")).expect("mkdir target");
        seed_data(&root);
        let exe = root.join("target/release/flutterdec");

        let layout = Layout::resolve_with(&exe, &env(&[("HOME", td.path().to_str().unwrap())]))
            .expect("resolve");
        assert_eq!(layout.data_source(), DataSource::BuildTree);
        assert!(layout.registry_path().is_file());
    }

    #[test]
    fn nothing_is_resolved_from_the_current_directory() {
        let td = tempdir().expect("tempdir");
        seed_data(td.path());
        // The data is *here*, which is exactly what discovery must ignore.
        let elsewhere = td.path().join("no/such/prefix/bin/flutterdec");

        let err = Layout::resolve_with(
            &elsewhere,
            &env(&[("HOME", td.path().to_str().unwrap())]),
        )
        .expect_err("the current directory is not a discovery input");
        let LayoutError::NoDataDirectory { candidates } = err else {
            panic!("wrong error: {err}");
        };
        assert_eq!(candidates.len(), 3);
    }

    #[test]
    fn xdg_data_home_places_the_store_and_an_override_replaces_it() {
        let td = tempdir().expect("tempdir");
        let data = td.path().join("packaged");
        seed_data(&data);
        let exe = td.path().join("bin/flutterdec");

        let layout = Layout::resolve_with(
            &exe,
            &env(&[
                (DATA_DIR_VAR, data.to_str().unwrap()),
                ("XDG_DATA_HOME", "/xdg"),
                ("HOME", "/home/ignored"),
            ]),
        )
        .expect("resolve");
        assert_eq!(layout.store_dir(), Path::new("/xdg/flutterdec/adapters"));

        let layout = Layout::resolve_with(
            &exe,
            &env(&[
                (DATA_DIR_VAR, data.to_str().unwrap()),
                ("XDG_DATA_HOME", "/xdg"),
                (STORE_DIR_VAR, "/explicit/store"),
                (SYMBOL_CACHE_VAR, "/explicit/symbols"),
            ]),
        )
        .expect("resolve");
        assert_eq!(layout.store_dir(), Path::new("/explicit/store"));
        assert_eq!(layout.symbols_dir(), Path::new("/explicit/symbols"));
    }

    #[test]
    fn a_missing_data_home_is_an_error_rather_than_a_guess() {
        let td = tempdir().expect("tempdir");
        let data = td.path().join("packaged");
        seed_data(&data);
        let err = Layout::resolve_with(
            &td.path().join("bin/flutterdec"),
            &env(&[(DATA_DIR_VAR, data.to_str().unwrap())]),
        )
        .expect_err("no HOME and no store override");
        assert!(matches!(err, LayoutError::NoDataHome { .. }), "{err}");
    }
}
