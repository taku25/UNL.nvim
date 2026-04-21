//! Git-based VCS provider using `git` CLI commands.
//!
//! Deliberately uses `std::process::Command` rather than libgit2/gitoxide to
//! keep the binary lightweight and avoid C FFI link issues on Windows.

use std::path::{Path, PathBuf};
use std::collections::HashSet;
use std::process::Command;
use super::{VcsProvider, ChangedFiles};

pub struct GitProvider;

impl VcsProvider for GitProvider {
    fn name(&self) -> &'static str { "git" }

    fn current_revision(&self, root: &Path) -> Option<String> {
        let out = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(root)
            .output()
            .ok()?;
        if out.status.success() {
            Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            None
        }
    }

    /// Returns files changed between `since_rev` and HEAD.
    ///
    /// Uses `git diff --name-status` for committed changes plus
    /// `git ls-files --others --exclude-standard` for untracked new files.
    fn changed_since(&self, root: &Path, since_rev: &str) -> Option<ChangedFiles> {
        // --- committed diff ---
        let diff_out = Command::new("git")
            .args(["diff", "--name-status", "--no-renames", since_rev, "HEAD"])
            .current_dir(root)
            .output()
            .ok()?;
        if !diff_out.status.success() { return None; }

        let mut modified: HashSet<PathBuf> = HashSet::new();
        let mut deleted:  HashSet<PathBuf> = HashSet::new();

        for line in String::from_utf8_lossy(&diff_out.stdout).lines() {
            // Format: <status>\t<path>
            // Status chars: A=added, M=modified, D=deleted, T=type-changed, U=unmerged
            let mut parts = line.splitn(2, '\t');
            let status = parts.next().unwrap_or("").trim();
            let rel    = match parts.next() { Some(p) => p.trim(), None => continue };
            let abs    = root.join(rel);
            if status.starts_with('D') {
                deleted.insert(abs);
            } else {
                modified.insert(abs);
            }
        }

        // --- untracked files (new files not yet committed) ---
        let untracked_out = Command::new("git")
            .args(["ls-files", "--others", "--exclude-standard"])
            .current_dir(root)
            .output();
        if let Ok(out) = untracked_out {
            if out.status.success() {
                for line in String::from_utf8_lossy(&out.stdout).lines() {
                    let rel = line.trim();
                    if !rel.is_empty() {
                        modified.insert(root.join(rel));
                    }
                }
            }
        }

        Some(ChangedFiles { modified, deleted })
    }
}
