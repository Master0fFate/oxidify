use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;

use crate::api::models::{Page, Track};

#[derive(Default)]
pub(super) struct Writes {
    latest: std::sync::atomic::AtomicU64,
    lock: tokio::sync::Mutex<()>,
}

impl Writes {
    pub(super) fn begin(&self) -> u64 {
        self.latest
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1
    }

    pub(super) async fn store(&self, epoch: u64, root: &Path, account: &str, page: Page<Track>) {
        let _guard = self.lock.lock().await;
        if self.latest.load(std::sync::atomic::Ordering::SeqCst) == epoch {
            store(root, account, page).await;
        }
    }
}

const TTL_SECS: u64 = 6 * 60 * 60;
const VERSION: u32 = 1;
const MAX_BYTES: usize = 1024 * 1024;

#[derive(Deserialize, Serialize)]
struct Entry {
    version: u32,
    account: String,
    saved_at: u64,
    page: Page<Track>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn path(root: &Path) -> PathBuf {
    root.join("top-tracks.json")
}

fn decode(bytes: &[u8], account: &str, now: u64) -> Option<Page<Track>> {
    if bytes.len() > MAX_BYTES {
        return None;
    }
    let entry: Entry = serde_json::from_slice(bytes).ok()?;
    (entry.version == VERSION
        && entry.account == account
        && now.checked_sub(entry.saved_at)? < TTL_SECS
        && entry.page.offset == 0
        && entry.page.items.len() <= 50)
        .then_some(entry.page)
}

pub(super) async fn load(root: &Path, account: &str) -> Option<Page<Track>> {
    let file = tokio::fs::File::open(path(root)).await.ok()?;
    let mut bytes = Vec::new();
    file.take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .ok()?;
    decode(&bytes, account, now())
}

pub(super) async fn store(root: &Path, account: &str, page: Page<Track>) {
    let entry = Entry {
        version: VERSION,
        account: account.to_owned(),
        saved_at: now(),
        page,
    };
    let Ok(bytes) = serde_json::to_vec(&entry) else {
        return;
    };
    if bytes.len() > MAX_BYTES {
        return;
    }
    let path = path(root);
    let Some(parent) = path.parent() else {
        return;
    };
    if tokio::fs::create_dir_all(parent).await.is_err() {
        return;
    }
    // Concurrent refreshes must not share a temporary file.
    let temporary = path.with_extension(format!("{}.tmp", rand::random::<u64>()));
    if tokio::fs::write(&temporary, bytes).await.is_ok() {
        let _ = tokio::fs::rename(&temporary, &path).await;
    }
    let _ = tokio::fs::remove_file(temporary).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn older_completion_cannot_overwrite_refreshed_cache() {
        let root =
            std::env::temp_dir().join(format!("oxidify-cache-epoch-{}", rand::random::<u64>()));
        let writes = Writes::default();
        let old = writes.begin();
        let fresh = writes.begin();
        let mut page = entry().page;
        page.total = 2;
        writes.store(fresh, &root, "alice", page).await;
        writes.store(old, &root, "alice", entry().page).await;
        assert_eq!(load(&root, "alice").await.unwrap().total, 2);
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    fn entry() -> Entry {
        Entry {
            version: VERSION,
            account: "alice".into(),
            saved_at: 100,
            page: Page {
                items: vec![Track::default()],
                limit: 50,
                total: 1,
                ..Default::default()
            },
        }
    }

    #[test]
    fn cache_requires_matching_account_version_and_fresh_timestamp() {
        let mut entry = entry();
        let bytes = serde_json::to_vec(&entry).unwrap();
        assert!(decode(&bytes, "alice", 100).is_some());
        assert!(decode(&bytes, "alice", 100 + TTL_SECS - 1).is_some());
        assert!(decode(&bytes, "alice", 100 + TTL_SECS).is_none());
        assert!(decode(&bytes, "alice", 99).is_none());
        assert!(decode(&bytes, "bob", 100).is_none());
        assert!(decode(b"broken", "alice", 100).is_none());
        entry.version += 1;
        assert!(decode(&serde_json::to_vec(&entry).unwrap(), "alice", 100).is_none());
    }

    #[test]
    fn oversized_entries_are_rejected() {
        assert!(decode(&vec![b' '; MAX_BYTES + 1], "alice", 100).is_none());
    }

    #[test]
    fn only_first_pages_of_at_most_fifty_tracks_are_accepted() {
        let mut entry = entry();
        entry.page.offset = 50;
        assert!(decode(&serde_json::to_vec(&entry).unwrap(), "alice", 100).is_none());
        entry.page.offset = 0;
        entry.page.items = vec![Track::default(); 51];
        assert!(decode(&serde_json::to_vec(&entry).unwrap(), "alice", 100).is_none());
    }

    #[tokio::test]
    async fn oversized_disk_reads_and_writes_are_ignored() {
        let root =
            std::env::temp_dir().join(format!("oxidify-top-tracks-{}", rand::random::<u64>()));
        let original = entry().page;
        store(&root, "alice", original.clone()).await;
        let mut oversized = original.clone();
        oversized.items[0].name = "x".repeat(MAX_BYTES);
        store(&root, "alice", oversized).await;
        assert_eq!(load(&root, "alice").await, Some(original));
        tokio::fs::write(path(&root), vec![b' '; MAX_BYTES + 1])
            .await
            .unwrap();
        assert!(load(&root, "alice").await.is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn disk_round_trip_replaces_existing_cache_without_cross_account_reads() {
        let root =
            std::env::temp_dir().join(format!("oxidify-top-tracks-{}", rand::random::<u64>()));
        let first = entry().page;
        store(&root, "alice", first.clone()).await;
        assert_eq!(load(&root, "alice").await, Some(first));
        assert!(load(&root, "bob").await.is_none());
        let second = Page::default();
        store(&root, "alice", second.clone()).await;
        assert_eq!(load(&root, "alice").await, Some(second));
        store(&root, "bob", entry().page).await;
        assert!(load(&root, "alice").await.is_none());
        assert!(load(&root, "bob").await.is_some());
        let entries = std::fs::read_dir(&root).unwrap();
        assert_eq!(entries.count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }
}
