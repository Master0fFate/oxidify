use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;

use crate::api::models::PlaylistItem;

pub(super) const VERSION: u32 = 2;
pub(super) const MAX_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct CachedPlaylist {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub account: String,
    pub snapshot: String,
    pub items: Vec<PlaylistItem>,
}

pub(super) fn file_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.json"))
}

pub(super) fn decode(bytes: &[u8], account: &str) -> Option<CachedPlaylist> {
    if bytes.len() > MAX_BYTES {
        return None;
    }
    let entry = serde_json::from_slice::<CachedPlaylist>(bytes).ok()?;
    let version_ok = entry.version == VERSION || entry.version == 0;
    let account_ok = entry.account.is_empty() || entry.account == account;
    (version_ok && account_ok && !entry.snapshot.is_empty()).then_some(entry)
}

pub(super) async fn load(dir: &Path, account: &str, id: &str) -> Option<CachedPlaylist> {
    let file = tokio::fs::File::open(file_path(dir, id)).await.ok()?;
    let mut bytes = Vec::new();
    file.take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .ok()?;
    decode(&bytes, account)
}

pub(super) async fn store(
    dir: &Path,
    account: &str,
    id: &str,
    snapshot: String,
    items: Vec<PlaylistItem>,
) {
    let entry = CachedPlaylist {
        version: VERSION,
        account: account.to_owned(),
        snapshot,
        items,
    };
    let Ok(bytes) = serde_json::to_vec(&entry) else {
        return;
    };
    if bytes.len() > MAX_BYTES {
        return;
    }
    if tokio::fs::create_dir_all(dir).await.is_err() {
        return;
    }
    let path = file_path(dir, id);
    let temporary = path.with_extension(format!("{}.tmp", rand::random::<u64>()));
    if tokio::fs::write(&temporary, bytes).await.is_ok() {
        let _ = tokio::fs::rename(&temporary, &path).await;
    }
    let _ = tokio::fs::remove_file(temporary).await;
}

pub(super) async fn clear_dir(root: &Path) -> std::io::Result<u64> {
    if !root.exists() {
        return Ok(0);
    }
    let mut removed = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries = tokio::fs::read_dir(&dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let meta = entry.metadata().await?;
            if meta.is_dir() {
                stack.push(path);
            } else {
                removed = removed.saturating_add(meta.len());
                let _ = tokio::fs::remove_file(path).await;
            }
        }
    }
    let _ = tokio::fs::remove_dir_all(root).await;
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(account: &str) -> CachedPlaylist {
        CachedPlaylist {
            version: VERSION,
            account: account.into(),
            snapshot: "snap-1".into(),
            items: vec![PlaylistItem::default()],
        }
    }

    #[test]
    fn decode_accepts_legacy_snapshot_items() {
        let legacy = serde_json::json!({
            "snapshot": "old",
            "items": [{}]
        });
        let bytes = serde_json::to_vec(&legacy).unwrap();
        let cached = decode(&bytes, "alice").unwrap();
        assert_eq!(cached.snapshot, "old");
        assert_eq!(cached.items.len(), 1);
    }

    #[test]
    fn decode_rejects_wrong_account_and_oversize() {
        let bytes = serde_json::to_vec(&sample("alice")).unwrap();
        assert!(decode(&bytes, "alice").is_some());
        assert!(decode(&bytes, "bob").is_none());
        assert!(decode(&vec![b' '; MAX_BYTES + 1], "alice").is_none());
        assert!(decode(b"broken", "alice").is_none());
    }

    #[tokio::test]
    async fn store_round_trip_and_cap() {
        let root = std::env::temp_dir().join(format!("oxidify-pl-cache-{}", rand::random::<u64>()));
        let dir = root.join("alice");
        store(
            &dir,
            "alice",
            "pl",
            "snap-1".into(),
            vec![PlaylistItem::default()],
        )
        .await;
        let loaded = load(&dir, "alice", "pl").await.unwrap();
        assert_eq!(loaded.snapshot, "snap-1");
        assert_eq!(loaded.items.len(), 1);

        let huge = vec![PlaylistItem {
            added_at: Some("x".repeat(MAX_BYTES)),
            ..PlaylistItem::default()
        }];
        store(&dir, "alice", "pl", "snap-2".into(), huge).await;
        let loaded = load(&dir, "alice", "pl").await.unwrap();
        assert_eq!(loaded.snapshot, "snap-1");

        let bytes = clear_dir(&root).await.unwrap();
        assert!(bytes > 0);
        assert!(load(&dir, "alice", "pl").await.is_none());
        let _ = tokio::fs::remove_dir_all(root).await;
    }
}
