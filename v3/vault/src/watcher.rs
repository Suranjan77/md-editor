//! Debounced filesystem watcher (plan §3.4): `notify` events are batched in
//! a 500 ms quiet window and delivered as one deduplicated path set, so a
//! burst of writes (editor save, `git checkout`, sync tools) triggers one
//! re-index pass, not dozens. Consumers feed the batch straight into
//! [`crate::index::SearchIndex::sync_paths`].

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher as _};

use crate::error::VaultError;

pub const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(500);

/// Watches a vault root; emits deduplicated path batches after a debounce
/// quiet window. Dropping the watcher stops everything.
pub struct VaultWatcher {
    // Order matters on drop: watcher first (stops the event source).
    _watcher: notify::RecommendedWatcher,
    batches: mpsc::Receiver<Vec<PathBuf>>,
}

impl VaultWatcher {
    pub fn new(root: &Path, debounce: Duration) -> Result<VaultWatcher, VaultError> {
        let (raw_tx, raw_rx) = mpsc::channel::<Vec<PathBuf>>();
        let (batch_tx, batch_rx) = mpsc::channel::<Vec<PathBuf>>();

        let mut watcher =
            notify::recommended_watcher(move |event: Result<notify::Event, notify::Error>| {
                // Access (open/close-on-read) events must be dropped:
                // consumers *read* files in response to a batch, so
                // forwarding reads would feed the watcher its own echo —
                // an infinite reindex loop.
                if let Ok(event) = event
                    && !event.paths.is_empty()
                    && !matches!(event.kind, notify::EventKind::Access(_))
                {
                    let _ = raw_tx.send(event.paths);
                }
            })
            .map_err(|e| VaultError::watch(root, e))?;
        watcher
            .watch(root, RecursiveMode::Recursive)
            .map_err(|e| VaultError::watch(root, e))?;

        // Debounce thread: first event opens a window; everything arriving
        // within `debounce` of the *last* event joins the batch.
        std::thread::spawn(move || {
            while let Ok(first) = raw_rx.recv() {
                let mut batch: BTreeSet<PathBuf> = first.into_iter().collect();
                let mut deadline = Instant::now() + debounce;
                loop {
                    let now = Instant::now();
                    let Some(remaining) = deadline
                        .checked_duration_since(now)
                        .filter(|d| !d.is_zero())
                    else {
                        break;
                    };
                    match raw_rx.recv_timeout(remaining) {
                        Ok(more) => {
                            batch.extend(more);
                            deadline = Instant::now() + debounce;
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => break,
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }
                if batch_tx.send(batch.into_iter().collect()).is_err() {
                    return;
                }
            }
        });

        Ok(VaultWatcher {
            _watcher: watcher,
            batches: batch_rx,
        })
    }

    /// Next debounced batch, if one is ready.
    pub fn try_next(&self) -> Option<Vec<PathBuf>> {
        self.batches.try_recv().ok()
    }

    /// Block up to `timeout` for the next batch.
    pub fn next_timeout(&self, timeout: Duration) -> Option<Vec<PathBuf>> {
        self.batches.recv_timeout(timeout).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::SearchIndex;

    fn ok<T, E: std::fmt::Display>(r: Result<T, E>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("{e}"),
        }
    }

    /// The plan's M2 gate: external edit → converged index in under 2 s.
    #[test]
    fn external_edit_converges_in_under_two_seconds() {
        let dir = ok(tempfile::tempdir());
        let root = match dir.path().canonicalize() {
            Ok(p) => p,
            Err(e) => panic!("canonicalize: {e}"),
        };
        let mut index = ok(SearchIndex::open_in_memory());
        ok(index.sync(&root));

        let watcher = ok(VaultWatcher::new(&root, Duration::from_millis(150)));
        let started = Instant::now();

        // "External" edits: two files written in quick succession.
        ok(std::fs::write(root.join("one.md"), "alpha contents"));
        ok(std::fs::write(root.join("two.md"), "beta contents"));

        let Some(batch) = watcher.next_timeout(Duration::from_secs(5)) else {
            panic!("no watcher batch arrived");
        };
        ok(index.sync_paths(&root, &batch));
        // The burst may straggle across batches; drain whatever follows.
        while let Some(more) = watcher.next_timeout(Duration::from_millis(300)) {
            ok(index.sync_paths(&root, &more));
        }

        assert_eq!(ok(index.search("alpha", 10)).len(), 1);
        assert_eq!(ok(index.search("beta", 10)).len(), 1);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "convergence took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn burst_of_writes_is_one_batch() {
        let dir = ok(tempfile::tempdir());
        let root = match dir.path().canonicalize() {
            Ok(p) => p,
            Err(e) => panic!("canonicalize: {e}"),
        };
        let watcher = ok(VaultWatcher::new(&root, Duration::from_millis(200)));
        for i in 0..5 {
            ok(std::fs::write(root.join(format!("n{i}.md")), "x"));
        }
        let Some(batch) = watcher.next_timeout(Duration::from_secs(5)) else {
            panic!("no batch");
        };
        let unique: BTreeSet<_> = batch.iter().collect();
        assert!(
            unique.len() >= 5,
            "expected all 5 files in one debounced batch, got {batch:?}"
        );
    }
}
