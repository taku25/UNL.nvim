//! Perforce (p4) VCS provider.
//!
//! Uses the `p4` CLI. No libp4api FFI — lightweight and cross-platform.
//!
//! Revision token  : the highest submitted changelist number visible in the
//!                   workspace mapped to `root` (e.g. `"12345"`).
//! changed_since   : `p4 fstat` with a server-side `-F headChange>PREV_CL`
//!                   filter — returns only files whose head revision was
//!                   submitted after the stored changelist.

use std::path::Path;
use std::collections::HashSet;
use std::process::Command;
use super::{VcsProvider, ChangedFiles};

pub struct P4Provider;

impl VcsProvider for P4Provider {
    fn name(&self) -> &'static str { "perforce" }

    /// Returns the highest submitted CL number as a string.
    ///
    /// Runs: `p4 changes -m1 -s submitted ROOT/...`
    /// Output line format: `Change 12345 on 2024/01/01 by user@client 'desc'`
    fn current_revision(&self, root: &Path) -> Option<String> {
        let depot_spec = format!("{}/...", root.to_string_lossy().replace('\\', "/"));
        let out = Command::new("p4")
            .args(["changes", "-m1", "-s", "submitted", &depot_spec])
            .current_dir(root)
            .output()
            .ok()?;
        if !out.status.success() { return None; }
        parse_cl_from_changes(&String::from_utf8_lossy(&out.stdout))
    }

    /// Returns files added/edited/deleted after `since_rev` (a CL number string).
    ///
    /// Runs: `p4 fstat -T localPath,headAction -F headChange>PREV_CL ROOT/...`
    ///
    /// fstat output block per file:
    /// ```
    /// ... localPath C:/work/proj/Source/Foo.cpp
    /// ... headAction edit
    /// ```
    fn changed_since(&self, root: &Path, since_rev: &str) -> Option<ChangedFiles> {
        let since_cl: i64 = since_rev.parse().ok()?;
        let depot_spec = format!("{}/...", root.to_string_lossy().replace('\\', "/"));
        let filter = format!("headChange>{}", since_cl);

        let out = Command::new("p4")
            .args(["fstat", "-T", "localPath,headAction", "-F", &filter, &depot_spec])
            .current_dir(root)
            .output()
            .ok()?;
        if !out.status.success() { return None; }

        let mut modified = HashSet::new();
        let mut deleted  = HashSet::new();
        let text = String::from_utf8_lossy(&out.stdout);

        // Parse blocks separated by blank lines.
        // Within each block collect localPath and headAction.
        let mut local_path: Option<std::path::PathBuf> = None;
        let mut head_action: Option<String> = None;

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                // flush block
                if let (Some(p), Some(a)) = (local_path.take(), head_action.take()) {
                    if a.contains("delete") || a.contains("purge") || a.contains("archive") {
                        deleted.insert(p);
                    } else {
                        modified.insert(p);
                    }
                }
                continue;
            }
            if let Some(rest) = line.strip_prefix("... localPath ") {
                local_path = Some(std::path::PathBuf::from(rest.trim()));
            } else if let Some(rest) = line.strip_prefix("... headAction ") {
                head_action = Some(rest.trim().to_string());
            }
        }
        // flush last block (no trailing blank line)
        if let (Some(p), Some(a)) = (local_path, head_action) {
            if a.contains("delete") || a.contains("purge") || a.contains("archive") {
                deleted.insert(p);
            } else {
                modified.insert(p);
            }
        }

        Some(ChangedFiles { modified, deleted })
    }
}

/// Attempt to detect an active P4 workspace at `root`.
/// Returns the highest submitted CL if p4 is available and the directory is
/// inside a mapped workspace, or `None` otherwise.
pub fn detect_revision(root: &Path) -> Option<String> {
    P4Provider.current_revision(root)
}

fn parse_cl_from_changes(output: &str) -> Option<String> {
    // "Change 12345 on ..."
    for line in output.lines() {
        let mut parts = line.split_whitespace();
        if parts.next() == Some("Change") {
            if let Some(cl) = parts.next() {
                if cl.parse::<u64>().is_ok() {
                    return Some(cl.to_string());
                }
            }
        }
    }
    None
}
