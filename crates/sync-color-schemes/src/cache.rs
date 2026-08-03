//! Lightweight embedded HTTP-response cache backed by [`redb`].
//!
//! Replaces the former `sqlite-cache` dependency (which pulled in
//! `rusqlite` + the bundled C `libsqlite3-sys`). `redb` is a pure-Rust,
//! ACID, B-tree key-value store with no `unsafe` in its core.
//!
//! Each entry is stored under the key `"{topic}|{key}"` and its value is the
//! serialization `(expires_at_unix_secs: u64, big-endian) || blob`. TTL is
//! enforced on read: an entry whose expiry has elapsed is reported as a miss
//! and removed, so the caller re-fetches the resource.

use anyhow::{Context, Result};
use redb::{Database, TableDefinition};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Single table holding every cached blob, keyed by `"{topic}|{key}"`.
const TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("data-by-url");

/// Number of leading bytes used to store the big-endian expiry timestamp.
const EXPIRY_LEN: usize = std::mem::size_of::<u64>();

/// Embedded redb-backed cache.
///
/// Cloning is cheap: it only bumps the refcount on the shared [`Database`]
/// handle, so a `Topic` (and the [`KeyUpdater`] it yields) can outlive the
/// borrow that created it.
#[derive(Clone)]
pub struct Cache {
    db: Arc<Database>,
}

/// A namespaced view over the cache. All keys are transparently prefixed with
/// the topic name so multiple topics never collide.
pub struct Topic {
    db: Arc<Database>,
    topic: String,
}

/// A cached value returned to the caller.
pub struct Value {
    /// The cached blob.
    pub data: Vec<u8>,
}

/// Handle returned by [`Topic::get_for_update`]. Calling [`KeyUpdater::write`]
/// stores the freshly fetched value under the original key.
pub struct KeyUpdater {
    db: Arc<Database>,
    storage_key: String,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Builds the composite storage key that folds the topic namespace in.
fn storage_key_for(topic: &str, key: &str) -> String {
    format!("{topic}|{key}")
}

fn encode(expires_at: u64, data: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(EXPIRY_LEN + data.len());
    buf.extend_from_slice(&expires_at.to_be_bytes());
    buf.extend_from_slice(data);
    buf
}

/// Splits a stored value into `(expires_at, blob)`. Returns `None` if the
/// entry is too short to contain a timestamp (e.g. corruption).
fn decode(bytes: &[u8]) -> Option<(u64, &[u8])> {
    if bytes.len() < EXPIRY_LEN {
        return None;
    }
    let (head, blob) = bytes.split_at(EXPIRY_LEN);
    let mut arr = [0u8; EXPIRY_LEN];
    arr.copy_from_slice(head);
    Some((u64::from_be_bytes(arr), blob))
}

impl Cache {
    /// Opens (creating if necessary) the redb database at `path` and ensures
    /// the cache table exists.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let db = Database::create(path)
            .with_context(|| format!("opening redb cache {}", path.display()))?;
        // Create the table up front so reads never have to.
        let txn = db.begin_write().context("opening redb write transaction")?;
        {
            let _ = txn
                .open_table(TABLE)
                .context("creating/opening redb cache table")?;
        }
        txn.commit()
            .context("committing redb cache table initialization")?;
        Ok(Self { db: Arc::new(db) })
    }

    /// Returns a [`Topic`] scoped to `topic`. The topic name is folded into
    /// every stored key, so distinct topics never collide.
    pub fn topic(&self, topic: &str) -> Result<Topic> {
        Ok(Topic {
            db: self.db.clone(),
            topic: topic.to_string(),
        })
    }
}

impl Topic {
    /// Looks up `key`, returning the cached value if a fresh (non-expired)
    /// entry exists. Expired entries are removed and reported as `None`.
    pub fn get(&self, key: &str) -> Result<Option<Value>> {
        read_fresh(&self.db, &storage_key_for(&self.topic, key))
    }

    /// Stores `value` under `key`, expiring after `ttl`.
    pub fn set(&self, key: &str, value: &[u8], ttl: Duration) -> Result<()> {
        write_entry(&self.db, &storage_key_for(&self.topic, key), value, ttl)
    }

    /// Looks up `key`, returning a [`KeyUpdater`] plus the cached value if a
    /// fresh (non-expired) entry exists.
    ///
    /// Expired entries are removed and reported as `None` so the caller knows
    /// to re-fetch and then call [`KeyUpdater::write`].
    pub async fn get_for_update(&self, key: &str) -> Result<(KeyUpdater, Option<Value>)> {
        let storage_key = storage_key_for(&self.topic, key);
        let value = read_fresh(&self.db, &storage_key)?;
        Ok((
            KeyUpdater {
                db: self.db.clone(),
                storage_key,
            },
            value,
        ))
    }
}

impl KeyUpdater {
    /// Stores `data` under the looked-up key, expiring after `ttl`.
    pub fn write(self, data: &[u8], ttl: Duration) -> Result<()> {
        write_entry(&self.db, &self.storage_key, data, ttl)
    }
}

fn read_fresh(db: &Database, storage_key: &str) -> Result<Option<Value>> {
    let txn = db.begin_read().context("opening redb read transaction")?;
    let table = txn
        .open_table(TABLE)
        .context("opening redb cache table for read")?;
    let Some(guard) = table
        .get(storage_key)
        .with_context(|| format!("reading cache entry for {storage_key}"))?
    else {
        return Ok(None);
    };
    let raw = guard.value();
    let (expires_at, blob) = match decode(raw) {
        Some(v) => v,
        None => {
            // Malformed entry: drop it and treat as a miss.
            drop(guard);
            drop(table);
            drop(txn);
            remove_entry(db, storage_key)?;
            return Ok(None);
        }
    };
    if now_secs() >= expires_at {
        // Expired: garbage-collect the stale entry and report a miss.
        drop(guard);
        drop(table);
        drop(txn);
        remove_entry(db, storage_key)?;
        return Ok(None);
    }
    Ok(Some(Value {
        data: blob.to_vec(),
    }))
}

fn write_entry(db: &Database, storage_key: &str, data: &[u8], ttl: Duration) -> Result<()> {
    let expires_at = now_secs().saturating_add(ttl.as_secs());
    let encoded = encode(expires_at, data);
    let txn = db.begin_write().context("opening redb write transaction")?;
    {
        let mut table = txn
            .open_table(TABLE)
            .context("opening redb cache table for write")?;
        table
            .insert(storage_key, encoded.as_slice())
            .with_context(|| format!("writing cache entry for {storage_key}"))?;
    }
    txn.commit().context("committing redb cache write")?;
    Ok(())
}

fn remove_entry(db: &Database, storage_key: &str) -> Result<()> {
    let txn = db.begin_write().context("opening redb gc transaction")?;
    {
        let mut table = txn
            .open_table(TABLE)
            .context("opening redb cache table for gc")?;
        let _ = table
            .remove(storage_key)
            .with_context(|| format!("removing stale cache entry for {storage_key}"))?;
    }
    txn.commit().context("committing redb gc")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Opens a cache backed by a fresh temporary redb file.
    fn temp_cache() -> (Cache, tempfile::NamedTempFile) {
        let file = tempfile::NamedTempFile::new().expect("create temp file");
        // redb opens by path, so hand it the path of the temp file.
        let cache = Cache::open(file.path()).expect("open cache");
        (cache, file)
    }

    /// Mirrors the fetch/refresh decision made by `main::fetch_url`: a miss
    /// calls the (mock) network and records the value; a hit returns the
    /// cached body without touching the network counter.
    async fn fetch(cache: &Cache, url: &str, ttl: Duration, fetch_calls: &mut u32) -> Vec<u8> {
        let topic = cache.topic("data-by-url").expect("topic");
        let (updater, item) = topic.get_for_update(url).await.expect("get_for_update");
        if let Some(item) = item {
            return item.data;
        }
        *fetch_calls += 1;
        let data = format!("body-of-{url}").into_bytes();
        updater.write(&data, ttl).expect("write");
        data
    }

    #[tokio::test]
    async fn cache_hit_does_not_refetch() {
        let (cache, _file) = temp_cache();
        let url = "https://example.com/a";
        let mut calls = 0u32;

        // First call: cold miss -> must fetch.
        let first = fetch(&cache, url, Duration::from_secs(3600), &mut calls).await;
        assert_eq!(first, b"body-of-https://example.com/a");
        assert_eq!(calls, 1, "first lookup should have triggered a fetch");

        // Second call: entry is well within its TTL -> served from cache.
        let second = fetch(&cache, url, Duration::from_secs(3600), &mut calls).await;
        assert_eq!(second, first, "cache hit should return the stored body");
        assert_eq!(calls, 1, "a cache hit must not re-fetch");
    }

    #[tokio::test]
    async fn ttl_expiry_triggers_refetch() {
        let (cache, _file) = temp_cache();
        let url = "https://example.com/b";
        let mut calls = 0u32;

        // Seed the cache with an immediately-expired entry (TTL == 0 =>
        // expires_at == now, which `now_secs() >= expires_at` already treats
        // as expired on the very next read).
        fetch(&cache, url, Duration::ZERO, &mut calls).await;
        assert_eq!(calls, 1, "seeding write still counts as one fetch");

        // The next lookup must NOT see a valid cached value, and so must
        // re-fetch from the network.
        let refreshed = fetch(&cache, url, Duration::from_secs(3600), &mut calls).await;
        assert_eq!(refreshed, b"body-of-https://example.com/b");
        assert_eq!(calls, 2, "an expired entry must trigger a re-fetch");

        // And a third call now hits again thanks to the long TTL refresh.
        let _ = fetch(&cache, url, Duration::from_secs(3600), &mut calls).await;
        assert_eq!(calls, 2, "after refresh the entry is fresh again");
    }

    #[tokio::test]
    async fn get_for_update_then_get_is_consistent() {
        let (cache, _file) = temp_cache();
        let topic = cache.topic("data-by-url").expect("topic");

        let (updater, item) = topic.get_for_update("k1").await.expect("get_for_update");
        assert!(item.is_none(), "cold key must be a miss");
        updater
            .write(b"v1", Duration::from_secs(3600))
            .expect("write");

        // The synchronous `get` used by base16 sync must see the same value.
        let hit = topic.get("k1").expect("get").expect("should be a hit");
        assert_eq!(hit.data, b"v1");
    }

    #[test]
    fn set_and_get_roundtrip_with_ttl() {
        let (cache, _file) = temp_cache();
        let topic = cache.topic("default-branch").expect("topic");

        // Long TTL: visible.
        topic
            .set("repo-a", b"main", Duration::from_secs(3600))
            .expect("set");
        let hit = topic.get("repo-a").expect("get").expect("hit");
        assert_eq!(hit.data, b"main");

        // TTL == 0: already expired on read -> miss, and entry is dropped.
        topic.set("repo-b", b"master", Duration::ZERO).expect("set");
        assert!(
            topic.get("repo-b").expect("get").is_none(),
            "expired entry must be a miss"
        );
    }

    #[tokio::test]
    async fn different_keys_are_independent() {
        let (cache, _file) = temp_cache();
        let topic = cache.topic("data-by-url").expect("topic");

        let (updater, item) = topic.get_for_update("k1").await.expect("get_for_update");
        assert!(item.is_none(), "unrelated key must be a miss");
        updater
            .write(b"v1", Duration::from_secs(3600))
            .expect("write");

        let (_, k1_again) = topic.get_for_update("k1").await.expect("get_for_update k1");
        let (_, k2) = topic.get_for_update("k2").await.expect("get_for_update k2");
        assert_eq!(k1_again.unwrap().data, b"v1");
        assert!(k2.is_none(), "a different key must not see k1's value");
    }

    #[test]
    fn distinct_topics_do_not_collide() {
        let (cache, _file) = temp_cache();
        let t1 = cache.topic("topic-one").expect("topic");
        let t2 = cache.topic("topic-two").expect("topic");
        t1.set("same-key", b"from-one", Duration::from_secs(3600))
            .expect("set");
        assert_eq!(t1.get("same-key").expect("get").unwrap().data, b"from-one");
        assert!(
            t2.get("same-key").expect("get").is_none(),
            "topics must be isolated"
        );
    }

    #[test]
    fn encode_decode_roundtrip() {
        for (expires_at, blob) in [
            (0u64, &b""[..]),
            (u64::MAX, b"\x00\x01\x02"),
            (1_700_000_000, b"hello world"),
        ] {
            let encoded = encode(expires_at, blob);
            let (decoded_exp, decoded_blob) = decode(&encoded).expect("decode");
            assert_eq!(decoded_exp, expires_at);
            assert_eq!(decoded_blob, blob);
        }
    }
}
