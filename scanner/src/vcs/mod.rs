//! VCS abstraction layer for incremental refresh optimization.
//!
//! Provides a unified interface for Git, Perforce, SVN, and other VCS tools.
//! Used by `refresh.rs` to skip rescanning unchanged files/roots.

pub mod git;
pub mod none;
// TODO: pub mod perforce;  -- P4 integration (p4 changes / p4 fstat)
// TODO: pub mod svn;       -- SVN integration (svn info / svn diff)

use std::fs;
use std::path::{Path, PathBuf};
use std::collections::HashSet;

/// Files changed between two VCS revisions.
pub struct ChangedFiles {
    /// Added or modified files (absolute paths).
    pub modified: HashSet<PathBuf>,
    /// Deleted files (absolute paths).
    pub deleted: HashSet<PathBuf>,
}

/// Abstract VCS provider. Implementations must be Send + Sync so they can be
/// stored in Arc and used across Rayon threads.
pub trait VcsProvider: Send + Sync {
    fn name(&self) -> &'static str;

    /// Returns an opaque revision string for the repository root.
    ///
    /// - Git         → 40-char SHA (`git rev-parse HEAD`)
    /// - P4          → changelist number string
    /// - SVN         → revision number string
    /// - BuildVersion → `"ue:{Major}.{Minor}.{Patch}+{Branch}"` for binary engines
    /// - None        → always `None`
    fn current_revision(&self, root: &Path) -> Option<String>;

    /// Returns the set of files that changed between `since_rev` and the
    /// current HEAD, relative to `root`.  Returns `None` if the diff cannot
    /// be computed (e.g. `since_rev` is no longer reachable, or provider
    /// does not support diff — e.g. BuildVersion).
    fn changed_since(&self, root: &Path, since_rev: &str) -> Option<ChangedFiles>;
}

// ---------------------------------------------------------------------------
// Build.version fingerprint for binary (Epic Launcher) engine distributions
// ---------------------------------------------------------------------------

/// Provider that uses `Engine/Build/Build.version` as a stable revision
/// fingerprint.  The file is shipped read-only with every binary UE release
/// and never changes unless the engine itself is updated/reinstalled.
///
/// `changed_since` always returns `None` because a binary engine is treated
/// as fully immutable between installs — if the version string changes the
/// caller will trigger a full re-scan anyway.
pub struct BuildVersionProvider;

impl VcsProvider for BuildVersionProvider {
    fn name(&self) -> &'static str { "build_version" }

    fn current_revision(&self, root: &Path) -> Option<String> {
        // The .version file lives at <engine_root>/Engine/Build/Build.version
        // but the root passed in may already be the engine root *or* a parent.
        // Try both common locations.
        for candidate in &[
            root.join("Engine/Build/Build.version"),
            root.join("Build/Build.version"),
            root.join("Build.version"),
        ] {
            if let Ok(content) = fs::read_to_string(candidate) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
                    let major  = v["MajorVersion"].as_i64().unwrap_or(0);
                    let minor  = v["MinorVersion"].as_i64().unwrap_or(0);
                    let patch  = v["PatchVersion"].as_i64().unwrap_or(0);
                    let branch = v["BranchName"].as_str().unwrap_or("Unknown");
                    return Some(format!("ue:{major}.{minor}.{patch}+{branch}"));
                }
            }
        }
        None
    }

    /// Binary engines are immutable between reinstalls.
    /// If the fingerprint changes (reinstall/update) a full scan is triggered.
    /// Within the same version there is never a partial diff to compute.
    fn changed_since(&self, _root: &Path, _since_rev: &str) -> Option<ChangedFiles> { None }
}

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

/// Detect the best available revision source for the given directory.
///
/// Detection order:
///   1. Git (`.git` directory / file, or `git rev-parse` succeeds)
///   2. TODO: Perforce (P4CONFIG / `.p4config` present)
///   3. TODO: SVN (`.svn` present)
///   4. `Engine/Build/Build.version` (binary UE from Epic Launcher)
///   5. NoVcs (always returns `None` → mtime-based fallback)
pub fn detect(root: &Path) -> Box<dyn VcsProvider> {
    // Git: .git can be a directory (normal clone) or a file (submodule/worktree)
    if root.join(".git").exists() {
        return Box::new(git::GitProvider);
    }
    // Fallback: git rev-parse works even without a .git at root (sparse checkouts, etc.)
    if git::GitProvider.current_revision(root).is_some() {
        return Box::new(git::GitProvider);
    }
    // TODO: P4CONFIG / .p4config detection
    // TODO: .svn directory detection

    // Binary UE engine (Epic Launcher): use Build.version as a stable fingerprint.
    if BuildVersionProvider.current_revision(root).is_some() {
        return Box::new(BuildVersionProvider);
    }

    Box::new(none::NoVcs)
}
