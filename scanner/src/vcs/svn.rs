//! SVN (Subversion) VCS provider.
//!
//! Uses the `svn` CLI.
//!
//! Revision token : repository revision number as returned by `svn info`
//!                  (e.g. `"4321"`).
//! changed_since  : `svn diff --summarize -r PREV_REV:HEAD ROOT`
//!                  returns the set of modified/added/deleted paths.

use std::path::Path;
use std::collections::HashSet;
use std::process::Command;
use super::{VcsProvider, ChangedFiles};

pub struct SvnProvider;

impl VcsProvider for SvnProvider {
    fn name(&self) -> &'static str { "svn" }

    /// Returns the working-copy revision as a string.
    ///
    /// Runs: `svn info --show-item revision ROOT`
    /// Output: `4321\n`
    fn current_revision(&self, root: &Path) -> Option<String> {
        let out = Command::new("svn")
            .args(["info", "--show-item", "revision"])
            .arg(root)
            .output()
            .ok()?;
        if !out.status.success() { return None; }
        let rev = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if rev.parse::<u64>().is_ok() { Some(rev) } else { None }
    }

    /// Returns files changed between `since_rev` and HEAD.
    ///
    /// Runs: `svn diff --summarize -r PREV_REV:HEAD ROOT`
    ///
    /// Output line format (one file per line):
    /// ```
    /// M       /abs/path/to/file.cpp
    /// A       /abs/path/to/new.cpp
    /// D       /abs/path/to/gone.cpp
    /// ```
    /// Status chars: A=added, D=deleted, M=modified, C=conflicted, ?=unknown
    fn changed_since(&self, root: &Path, since_rev: &str) -> Option<ChangedFiles> {
        let rev_range = format!("{}:HEAD", since_rev);
        let out = Command::new("svn")
            .args(["diff", "--summarize", "-r", &rev_range])
            .arg(root)
            .output()
            .ok()?;
        if !out.status.success() { return None; }

        let mut modified = HashSet::new();
        let mut deleted  = HashSet::new();

        for line in String::from_utf8_lossy(&out.stdout).lines() {
            // Format: "<STATUS>       <path>" — status is 1–7 chars + spaces
            // The status column is 8 chars wide (including trailing space).
            if line.len() < 9 { continue; }
            let status = line[..8].trim();
            let path   = std::path::PathBuf::from(line[8..].trim());
            if status.starts_with('D') {
                deleted.insert(path);
            } else if !status.is_empty() {
                modified.insert(path);
            }
        }

        Some(ChangedFiles { modified, deleted })
    }
}
