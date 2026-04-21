//! No-op VCS provider used when no VCS is detected.
//!
//! Always returns `None` for all operations, causing the refresh to fall back
//! to the default mtime-based comparison.

use std::path::Path;
use super::{VcsProvider, ChangedFiles};

pub struct NoVcs;

impl VcsProvider for NoVcs {
    fn name(&self) -> &'static str { "none" }
    fn current_revision(&self, _root: &Path) -> Option<String> { None }
    fn changed_since(&self, _root: &Path, _since_rev: &str) -> Option<ChangedFiles> { None }
}
