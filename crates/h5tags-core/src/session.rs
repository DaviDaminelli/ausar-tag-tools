//! Every configured build, loaded and kept up to date (PRD §6.1, §6.2).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use h5_defs::Definitions;
pub use h5_module::index::Progress;
use h5_module::index::{Index, UpdateReport};

use crate::catalog::Build;
use crate::config::{BuildConfig, Config};
use crate::stored::TagCache;

/// Receives progress of long steps: the build's name and how far it is.
/// Called from worker threads. The CLI draws it; the MCP server ignores it.
pub type ProgressSink = Box<dyn Fn(&str, Progress) + Send + Sync>;

pub struct Session {
    cache_dir: PathBuf,
    builds: Vec<Build>,
    /// Shared by every build (PRD §13), and kept across `reindex`.
    tag_cache: Arc<TagCache>,
    progress: ProgressSink,
}

fn no_progress() -> ProgressSink {
    Box::new(|_, _| {})
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("build '{build}': can't read the deploy folder {path}: {source}")]
    Deploy { build: String, path: PathBuf, source: std::io::Error },
    #[error("build '{build}': can't read the definitions folder {path}: {source}")]
    Defs { build: String, path: PathBuf, source: std::io::Error },
}

/// A build name that isn't configured.
#[derive(Debug, PartialEq, Eq)]
pub struct UnknownBuild {
    pub name: String,
    pub available: Vec<String>,
}

/// Loads (or builds) one build's index and brings it up to date; saves the
/// cache when anything changed.
fn load_build(
    cfg: &BuildConfig,
    cache_dir: &Path,
    previous: Option<Index>,
    tag_cache: &Arc<TagCache>,
    force: &(dyn Fn(&str) -> bool + Sync),
    progress: &ProgressSink,
) -> Result<(Build, UpdateReport), SessionError> {
    let defs = Definitions::load(&cfg.defs).map_err(|source| SessionError::Defs {
        build: cfg.name.clone(),
        path: cfg.defs.clone(),
        source,
    })?;
    let cache = cache_dir.join(format!("{}.index", cfg.name));
    let previous = previous.or_else(|| Index::load(&cache));
    let report_progress = |p: Progress| progress(&cfg.name, p);
    let (index, report) = Index::update_with_progress(&cfg.deploy, previous, force, &report_progress)
        .map_err(|source| SessionError::Deploy { build: cfg.name.clone(), path: cfg.deploy.clone(), source })?;
    tracing::info!(build = %cfg.name, ?report, "index ready");
    if (report.read > 0 || report.removed > 0 || report.failed > 0)
        && let Err(e) = index.save(&cache)
    {
        tracing::warn!(cache = %cache.display(), error = %e, "couldn't save the index cache");
    }
    Ok((Build::new_sharing(cfg.name.clone(), cfg.clone(), index, defs, tag_cache.clone()), report))
}

impl Session {
    /// Loads every build of `config`.
    pub fn open(config: Config) -> Result<Session, SessionError> {
        Session::open_with_progress(config, no_progress())
    }

    /// [`Session::open`], reporting long steps (here and in later calls such
    /// as `reindex`) to `progress`.
    pub fn open_with_progress(config: Config, progress: ProgressSink) -> Result<Session, SessionError> {
        let cache_dir = config.cache_dir();
        let never = |_: &str| false;
        let tag_cache = Arc::new(TagCache::new(config.tag_cache_bytes()));
        let builds = config
            .builds
            .iter()
            .map(|cfg| load_build(cfg, &cache_dir, None, &tag_cache, &never, &progress).map(|(b, _)| b))
            .collect::<Result<_, _>>()?;
        Ok(Session { cache_dir, builds, tag_cache, progress })
    }

    /// A session over builds already in memory (tests, embedding).
    pub fn from_builds(cache_dir: PathBuf, builds: Vec<Build>) -> Session {
        assert!(!builds.is_empty(), "a session needs at least one build");
        Session { cache_dir, builds, tag_cache: Arc::default(), progress: no_progress() }
    }

    pub fn builds(&self) -> &[Build] {
        &self.builds
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Re-reads the modules of build `b` whose path matches `force`.
    pub fn reindex(&mut self, b: usize, force: &(dyn Fn(&str) -> bool + Sync)) -> Result<UpdateReport, SessionError> {
        let previous = std::mem::take(&mut self.builds[b].index);
        let cfg = self.builds[b].config.clone();
        let (build, report) =
            load_build(&cfg, &self.cache_dir, Some(previous), &self.tag_cache, force, &self.progress)?;
        self.builds[b] = build;
        Ok(report)
    }

    /// The build to label results with: its name when more than one is configured.
    pub fn label(&self, build: &Build) -> Option<String> {
        (self.builds.len() > 1).then(|| build.name.clone())
    }

    /// The position of the build named `name`; the first build if `None`.
    pub fn build_index(&self, name: Option<&str>) -> Result<usize, UnknownBuild> {
        let Some(name) = name else { return Ok(0) };
        self.builds.iter().position(|b| b.name == name).ok_or_else(|| UnknownBuild {
            name: name.to_owned(),
            available: self.builds.iter().map(|b| b.name.clone()).collect(),
        })
    }

    pub fn build(&self, name: Option<&str>) -> Result<&Build, UnknownBuild> {
        Ok(&self.builds[self.build_index(name)?])
    }

    pub fn has_build(&self, name: &str) -> bool {
        self.builds.iter().any(|b| b.name == name)
    }
}
