//! Change detection for the workspace tree.
//!
//! A fingerprint over the size and modification time of every tracked file is
//! cheap enough to recompute on a timer — no bytes are read and the trees
//! involved are small — and it needs no filesystem notification dependency,
//! which keeps the binary self contained and identical on every platform.

use std::path::Path;
use std::time::SystemTime;

use walkdir::WalkDir;

use crate::domain::config::WorkspaceConfig;
use crate::domain::workspace::Workspace;

/// Maximum entries walked before the fingerprint gives up and stays stable.
///
/// A workspace this large is pathological; refusing to walk it keeps the tick
/// bounded instead of turning the dashboard into a directory scanner.
const MAX_ENTRIES: usize = 4096;

/// Opaque summary of the tracked tree at one point in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Fingerprint(u64);

impl Fingerprint {
    /// Folds another fingerprint into this one.
    ///
    /// Combining per-file stamps is how a caller narrows "something moved" down
    /// to "these files moved" without keeping one fingerprint per file.
    #[must_use]
    pub fn combined(self, other: Self) -> Self {
        let mut hash = self.0;
        mix(&mut hash, &other.0.to_le_bytes());
        Self(hash)
    }
}

/// Fingerprint of a single file, from metadata already in hand.
///
/// The whole-tree fingerprint says that something moved; this one says which
/// file did. Taking it from metadata rather than from the path means a snapshot
/// of an already-loaded workspace costs no I/O at all — the documents carry
/// their own size and modification time.
pub fn stamp_of(size: u64, modified: Option<SystemTime>) -> Fingerprint {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    mix(&mut hash, &size.to_le_bytes());
    if let Some(modified) = modified
        && let Ok(since_epoch) = modified.duration_since(std::time::UNIX_EPOCH)
    {
        mix(&mut hash, &since_epoch.as_nanos().to_le_bytes());
    }
    Fingerprint(hash)
}

/// Fingerprint of a single file, read from disk.
pub fn file_stamp(path: &Path) -> Fingerprint {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    stamp(path, &mut hash);
    Fingerprint(hash)
}

/// Summarises everything the workspace reads from disk.
pub fn fingerprint(workspace: &Workspace) -> Fingerprint {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut entries = 0usize;

    for root in [workspace.specs_dir(), workspace.changes_dir()] {
        walk(&root, &mut hash, &mut entries);
    }

    for file in [
        workspace.design_path(),
        WorkspaceConfig::locate(&workspace.root).unwrap_or_default(),
    ] {
        stamp(&file, &mut hash);
    }

    Fingerprint(hash)
}

fn walk(root: &Path, hash: &mut u64, entries: &mut usize) {
    if !root.is_dir() {
        return;
    }

    for entry in WalkDir::new(root)
        .max_depth(12)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        *entries += 1;
        if *entries > MAX_ENTRIES {
            return;
        }
        // Directories count too: an empty new change directory is a change.
        stamp(entry.path(), hash);
    }
}

fn stamp(path: &Path, hash: &mut u64) {
    mix(hash, path.as_os_str().as_encoded_bytes());

    let Ok(metadata) = std::fs::metadata(path) else {
        // A path that vanished still has to alter the fingerprint.
        mix(hash, b"\0missing");
        return;
    };

    mix(hash, &metadata.len().to_le_bytes());

    if let Ok(modified) = metadata.modified()
        && let Ok(since_epoch) = modified.duration_since(std::time::UNIX_EPOCH)
    {
        mix(hash, &since_epoch.as_nanos().to_le_bytes());
    }
}

/// FNV-1a, chosen because it needs no dependency and collisions here cost a
/// missed reload rather than anything durable.
fn mix(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn scratch() -> std::path::PathBuf {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "openspectation-watch-{}-{unique}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("specs")).unwrap();
        std::fs::create_dir_all(root.join("changes")).unwrap();
        std::fs::write(root.join("specs/one.md"), "# One\n").unwrap();
        std::fs::write(root.join("openspec.yaml"), "version: 1\n").unwrap();
        root
    }

    #[test]
    fn a_stable_tree_keeps_its_fingerprint() {
        let root = scratch();
        let workspace = Workspace::load(&root).unwrap();

        assert_eq!(fingerprint(&workspace), fingerprint(&workspace));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_per_file_stamp_moves_only_for_its_own_file() {
        let root = scratch();
        let one = root.join("specs/one.md");
        let two = root.join("specs/two.md");
        std::fs::write(&two, "# Two\n").unwrap();

        let before = file_stamp(&one);
        std::fs::write(&two, "# Two, at length\n").unwrap();

        assert_eq!(file_stamp(&one), before);
        assert_ne!(file_stamp(&two), before);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_stamp_taken_from_metadata_answers_to_both_of_its_inputs() {
        let now = std::time::SystemTime::now();
        let later = now + std::time::Duration::from_secs(1);

        assert_eq!(stamp_of(10, Some(now)), stamp_of(10, Some(now)));
        assert_ne!(stamp_of(10, Some(now)), stamp_of(11, Some(now)));
        assert_ne!(stamp_of(10, Some(now)), stamp_of(10, Some(later)));
    }

    #[test]
    fn a_new_file_changes_the_fingerprint() {
        let root = scratch();
        let workspace = Workspace::load(&root).unwrap();
        let before = fingerprint(&workspace);

        std::fs::write(root.join("specs/two.md"), "# Two\n").unwrap();
        let after = fingerprint(&workspace);

        let _ = std::fs::remove_dir_all(&root);
        assert_ne!(before, after);
    }
}
