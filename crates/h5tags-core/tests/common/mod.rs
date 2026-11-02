//! Shared setup of the real-file tests: a session on `H5_DEPLOY` with the
//! definitions of `H5_DEFS` (default `reference/H5Tags/tags`).

use std::path::PathBuf;

use h5_module::testutil::TempDir;
use h5tags_core::{Config, Session};

/// A session on `H5_DEPLOY`, with its index cache in a folder deleted
/// afterwards (keep the `TempDir` alive as long as the session).
pub fn real_session(name: &str) -> (Session, TempDir) {
    let deploy = PathBuf::from(std::env::var("H5_DEPLOY").expect("set H5_DEPLOY to a folder with .module files"));
    let cache = TempDir::new(name);
    (Session::open(Config::single(deploy, defs_dir(), None, Some(cache.0.clone()))).unwrap(), cache)
}

/// `H5_DEFS`, or the repository's `reference/H5Tags/tags`.
pub fn defs_dir() -> PathBuf {
    std::env::var("H5_DEFS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reference/H5Tags/tags"))
}
