//! Watching the user's theme folder (themes decision in
//! `docs/porting-plan.md`): the `notify` watcher runs on its own thread,
//! marks the folder changed and wakes the UI, which reloads the themes.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use notify::{RecursiveMode, Watcher};

use super::session::Waker;

/// A watch on a folder; dropping it stops watching.
pub struct FolderWatch {
    changed: Arc<AtomicBool>,
    _watcher: notify::RecommendedWatcher,
}

impl FolderWatch {
    /// Watches `dir` (created if missing), calling `wake` after changes.
    ///
    /// # Errors
    /// Creating the folder or starting the watcher.
    pub fn new(dir: &Path, wake: Waker) -> Result<Self, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let changed = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&changed);
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if event.is_ok_and(|e| !e.kind.is_access()) {
                    flag.store(true, Ordering::Release);
                    wake();
                }
            })
            .map_err(|e| e.to_string())?;
        watcher
            .watch(dir, RecursiveMode::NonRecursive)
            .map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(Self {
            changed,
            _watcher: watcher,
        })
    }

    /// True once after the folder changed.
    pub fn take_changed(&self) -> bool {
        self.changed.swap(false, Ordering::AcqRel)
    }
}
