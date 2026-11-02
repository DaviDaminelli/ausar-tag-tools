//! What the index, the header scan and the definition check share: their
//! cache files (a magic, a format number, then postcard), and reading modules
//! in parallel.

use std::fs;
use std::io;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::Serialize;
use serde::de::DeserializeOwned;

/// A cache file's kind and shape: `magic` tells the kinds apart, `format`
/// is bumped whenever the stored types change.
pub struct CacheFile {
    pub magic: &'static [u8; 8],
    pub format: u32,
    /// For log lines: "index", "header scan".
    pub what: &'static str,
}

impl CacheFile {
    /// The cached value, or `None` (and a log line saying why) if it is
    /// missing, unreadable, corrupt or from another format: the caller rebuilds.
    pub fn load<T: DeserializeOwned>(&self, path: &Path) -> Option<T> {
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return None,
            Err(e) => {
                tracing::warn!(cache = %path.display(), error = %e, "{} cache unreadable; rebuilding", self.what);
                return None;
            }
        };
        let body = bytes
            .strip_prefix(self.magic.as_slice())
            .and_then(|rest| rest.split_first_chunk::<4>())
            .filter(|(format, _)| u32::from_le_bytes(**format) == self.format)
            .map(|(_, body)| body);
        let Some(body) = body else {
            tracing::info!(cache = %path.display(), "{} cache is from another format; rebuilding", self.what);
            return None;
        };
        match postcard::from_bytes(body) {
            Ok(value) => Some(value),
            Err(e) => {
                tracing::warn!(cache = %path.display(), error = %e, "{} cache is corrupt; rebuilding", self.what);
                None
            }
        }
    }

    /// Writes `value` atomically (temporary file, then rename).
    pub fn save<T: Serialize>(&self, path: &Path, value: &T) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut bytes = self.magic.to_vec();
        bytes.extend_from_slice(&self.format.to_le_bytes());
        bytes.extend(postcard::to_stdvec(value).map_err(io::Error::other)?);
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, bytes)?;
        fs::rename(&tmp, path)
    }
}

/// Runs `work` on each of `items` on all cores; `done(n)` after each one, from
/// any thread. Returns `(item, result)` pairs in any order. Each item
/// is one module: work never shares a file.
pub fn in_parallel<T: Send>(
    items: &[usize],
    work: &(dyn Fn(usize) -> T + Sync),
    done: &(dyn Fn(usize) + Sync),
) -> Vec<(usize, T)> {
    let next = AtomicUsize::new(0);
    let finished = AtomicUsize::new(0);
    let results = Mutex::new(Vec::with_capacity(items.len()));
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get()).min(items.len().max(1));
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                while let Some(&i) = items.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let result = work(i);
                    results.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((i, result));
                    done(finished.fetch_add(1, Ordering::Relaxed) + 1);
                }
            });
        }
    });
    results.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner)
}
