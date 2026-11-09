//! Short-lived shared state with expiry: refresh tokens and their
//! families (`auth.rs`). In-process by default (`memory://`); Redis when
//! `HARNESS_KV_URL=redis://...`, so several server processes behind one
//! address share it and it survives a restart of any of them.
//!
//! Nothing here is the system of record: losing it logs people out
//! (refresh tokens), never loses data.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, thiserror::Error)]
#[error("key-value store: {0}")]
pub struct KvError(String);

pub type Result<T> = std::result::Result<T, KvError>;

#[async_trait::async_trait]
pub trait Kv: Send + Sync {
    fn name(&self) -> &'static str;
    async fn get(&self, key: &str) -> Result<Option<String>>;
    /// Set with a time to live.
    async fn set(&self, key: &str, value: &str, ttl: Duration) -> Result<()>;
    async fn del(&self, key: &str) -> Result<()>;
}

/// Open the store `url` names: empty or `local://` (the master's own
/// database), `memory://` (this process only) or `redis://`.
pub async fn open(url: &str, local: &harness_store::Store) -> anyhow::Result<Arc<dyn Kv>> {
    let url = url.trim();
    if url.is_empty() || url == "local://" || url == "sqlite://" {
        return Ok(Arc::new(StoreKv::new(local.clone())));
    }
    if url == "memory://" || url == "memory" {
        return Ok(Arc::new(MemoryKv::default()));
    }
    if url.starts_with("redis://") || url.starts_with("redis+unix://") {
        return Ok(Arc::new(RedisKv::connect(url).await?));
    }
    if url.starts_with("rediss://") {
        anyhow::bail!(
            "HARNESS_KV_URL: rediss:// (TLS) is not supported yet; keep Redis on a private network and use redis://"
        );
    }
    anyhow::bail!("HARNESS_KV_URL must be local://, memory:// or redis://host:port/db")
}

/// Entries with expiry in the master database's `kv` table (the default):
/// one server, but logins and logouts survive a restart, which a purely
/// in-memory map would not.
pub struct StoreKv {
    store: harness_store::Store,
    sets: std::sync::atomic::AtomicU32,
}

const STORE_PREFIX: &str = "ttl.";

impl StoreKv {
    pub fn new(store: harness_store::Store) -> Self {
        Self {
            store,
            sets: Default::default(),
        }
    }

    fn key(k: &str) -> String {
        format!("{STORE_PREFIX}{k}")
    }
}

fn kv_err(e: harness_store::StoreError) -> KvError {
    KvError(e.to_string())
}

#[async_trait::async_trait]
impl Kv for StoreKv {
    fn name(&self) -> &'static str {
        "local"
    }

    async fn get(&self, key: &str) -> Result<Option<String>> {
        let k = Self::key(key);
        let v = self
            .store
            .call(move |s| s.kv_get(&k))
            .await
            .map_err(kv_err)?;
        let now = harness_core::now_ms();
        Ok(v.and_then(|v| {
            let (exp, val) = v.split_once('|')?;
            (exp.parse::<i64>().ok()? > now).then(|| val.to_string())
        }))
    }

    async fn set(&self, key: &str, value: &str, ttl: Duration) -> Result<()> {
        let now = harness_core::now_ms();
        let (k, v) = (
            Self::key(key),
            format!("{}|{value}", now + ttl.as_millis() as i64),
        );
        let sweep = self
            .sets
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .is_multiple_of(256);
        self.store
            .call(move |s| {
                s.kv_set(&k, &v)?;
                if sweep {
                    s.kv_sweep_expired(STORE_PREFIX, now)?;
                }
                Ok(())
            })
            .await
            .map_err(kv_err)
    }

    async fn del(&self, key: &str) -> Result<()> {
        let k = Self::key(key);
        self.store
            .call(move |s| s.kv_delete(&k).map(|_| ()))
            .await
            .map_err(kv_err)
    }
}

/// In-process map with expiry (the "fake Redis" of the Minimal template).
#[derive(Default)]
pub struct MemoryKv {
    map: Mutex<HashMap<String, (String, Instant)>>,
}

impl MemoryKv {
    fn sweep(map: &mut HashMap<String, (String, Instant)>, now: Instant) {
        // Cheap enough at this size; keeps the map from growing with
        // expired refresh tokens.
        if map.len() > 1024 {
            map.retain(|_, (_, until)| *until > now);
        }
    }
}

#[async_trait::async_trait]
impl Kv for MemoryKv {
    fn name(&self) -> &'static str {
        "memory"
    }

    async fn get(&self, key: &str) -> Result<Option<String>> {
        let now = Instant::now();
        let mut map = self.map.lock().unwrap_or_else(|p| p.into_inner());
        match map.get(key) {
            Some((v, until)) if *until > now => Ok(Some(v.clone())),
            Some(_) => {
                map.remove(key);
                Ok(None)
            }
            None => Ok(None),
        }
    }

    async fn set(&self, key: &str, value: &str, ttl: Duration) -> Result<()> {
        let now = Instant::now();
        let mut map = self.map.lock().unwrap_or_else(|p| p.into_inner());
        Self::sweep(&mut map, now);
        map.insert(key.to_string(), (value.to_string(), now + ttl));
        Ok(())
    }

    async fn del(&self, key: &str) -> Result<()> {
        self.map
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(key);
        Ok(())
    }
}

/// Redis through a reconnecting connection manager.
pub struct RedisKv {
    conn: redis::aio::ConnectionManager,
}

fn rerr(e: redis::RedisError) -> KvError {
    // The URL (and its password) never goes into the message.
    KvError(format!("redis: {}", e.kind_str()))
}

trait KindStr {
    fn kind_str(&self) -> String;
}

impl KindStr for redis::RedisError {
    fn kind_str(&self) -> String {
        format!("{:?}", self.kind())
    }
}

impl RedisKv {
    pub async fn connect(url: &str) -> anyhow::Result<Self> {
        let client = redis::Client::open(url).map_err(|e| {
            anyhow::anyhow!("HARNESS_KV_URL is not a valid Redis URL ({})", e.kind_str())
        })?;
        let cfg = redis::aio::ConnectionManagerConfig::new()
            .set_connection_timeout(Some(Duration::from_secs(5)))
            .set_response_timeout(Some(Duration::from_secs(5)));
        let conn = redis::aio::ConnectionManager::new_with_config(client, cfg)
            .await
            .map_err(|e| {
                anyhow::anyhow!("connecting to Redis (HARNESS_KV_URL): {}", e.kind_str())
            })?;
        Ok(Self { conn })
    }
}

#[async_trait::async_trait]
impl Kv for RedisKv {
    fn name(&self) -> &'static str {
        "redis"
    }

    async fn get(&self, key: &str) -> Result<Option<String>> {
        let mut c = self.conn.clone();
        redis::cmd("GET")
            .arg(key)
            .query_async(&mut c)
            .await
            .map_err(rerr)
    }

    async fn set(&self, key: &str, value: &str, ttl: Duration) -> Result<()> {
        let mut c = self.conn.clone();
        redis::cmd("SET")
            .arg(key)
            .arg(value)
            .arg("PX")
            .arg(ttl.as_millis().max(1) as u64)
            .query_async::<()>(&mut c)
            .await
            .map_err(rerr)
    }

    async fn del(&self, key: &str) -> Result<()> {
        let mut c = self.conn.clone();
        redis::cmd("DEL")
            .arg(key)
            .query_async::<()>(&mut c)
            .await
            .map_err(rerr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn memory_kv_expires() {
        let kv = MemoryKv::default();
        kv.set("a", "1", Duration::from_millis(30)).await.unwrap();
        assert_eq!(kv.get("a").await.unwrap().as_deref(), Some("1"));
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(kv.get("a").await.unwrap(), None);
        kv.set("b", "2", Duration::from_secs(60)).await.unwrap();
        kv.del("b").await.unwrap();
        assert_eq!(kv.get("b").await.unwrap(), None);
    }

    #[tokio::test]
    async fn store_kv_expires_and_sweeps() {
        let st = harness_store::Store::open_in_memory().unwrap();
        let kv = StoreKv::new(st.clone());
        kv.set("a", "x|y", Duration::from_millis(30)).await.unwrap();
        assert_eq!(kv.get("a").await.unwrap().as_deref(), Some("x|y"));
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(kv.get("a").await.unwrap(), None);
        assert_eq!(
            st.kv_sweep_expired(STORE_PREFIX, harness_core::now_ms())
                .unwrap(),
            1
        );
        st.kv_set("other", "1|kept").unwrap();
        assert_eq!(
            st.kv_sweep_expired(STORE_PREFIX, i64::MAX).unwrap(),
            0,
            "only our prefix"
        );
    }

    /// Against a real Redis: `HARNESS_TEST_REDIS_URL=redis://:pw@127.0.0.1:6379/0
    /// cargo test -p harness-master redis_live -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn redis_live() {
        let url = std::env::var("HARNESS_TEST_REDIS_URL").expect("HARNESS_TEST_REDIS_URL");
        let st = harness_store::Store::open_in_memory().unwrap();
        let kv = open(&url, &st).await.unwrap();
        assert_eq!(kv.name(), "redis");
        kv.set("rh-test.a", "1", Duration::from_millis(300))
            .await
            .unwrap();
        assert_eq!(kv.get("rh-test.a").await.unwrap().as_deref(), Some("1"));
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(kv.get("rh-test.a").await.unwrap(), None);
        kv.set("rh-test.b", "2", Duration::from_secs(5))
            .await
            .unwrap();
        kv.del("rh-test.b").await.unwrap();
        assert_eq!(kv.get("rh-test.b").await.unwrap(), None);
        // A wrong password fails at connect, without the URL in the message.
        let bad = url.replacen("://:", "://:wrong", 1);
        let e = open(&bad, &st).await.err().expect("refused").to_string();
        assert!(!e.contains("wrong"), "{e}");
    }

    #[tokio::test]
    async fn urls() {
        let st = harness_store::Store::open_in_memory().unwrap();
        assert_eq!(open("", &st).await.unwrap().name(), "local");
        assert_eq!(open("memory://", &st).await.unwrap().name(), "memory");
        assert!(open("rediss://x", &st).await.is_err());
        assert!(open("postgres://x", &st).await.is_err());
    }
}
