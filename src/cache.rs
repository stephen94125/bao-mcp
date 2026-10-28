//! Disposable, bounded discovery metadata. Credentials and backend responses never enter this DB.

use crate::{Error, Result, discovery::Snapshot};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MAX_SNAPSHOT_BYTES: usize = 512 * 1024;
const MAX_DATABASE_PAGES: usize = 8192; // 32 MiB at a 4096-byte page size.

#[derive(Clone)]
pub struct Cache(Arc<Inner>);
struct Inner {
    db: Mutex<Connection>,
    idle_ttl: u64,
    max_age: u64,
    max_entries: usize,
}

impl Cache {
    pub fn open(
        path: &Path,
        idle_ttl: Duration,
        max_age: Duration,
        max_entries: usize,
    ) -> Result<Self> {
        if idle_ttl.is_zero() || max_age.is_zero() || !(1..=10_000).contains(&max_entries) {
            return Err(Error::invalid(
                "Cache lifetimes and capacity must be positive and bounded",
            ));
        }
        reject_symlinks(path)?;
        // Resolve trusted parent aliases (macOS /var -> /private/var) before
        // SQLITE_OPEN_NOFOLLOW, while leaving the DB target itself unchecked by
        // canonicalize so a final symlink is still rejected at open time.
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let resolved = parent
            .canonicalize()
            .map_err(|_| cache_error())?
            .join(path.file_name().ok_or_else(cache_error)?);
        let path = resolved.as_path();
        if path.exists()
            && (!path.is_file()
                || std::fs::metadata(path).map_err(|_| cache_error())?.len()
                    > (MAX_DATABASE_PAGES * 4096) as u64)
        {
            return Err(cache_error());
        }
        // Pre-create privately; SQLite's NOFOLLOW also rejects a replaced final symlink.
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(path) {
            Ok(_) => (),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(_) => return Err(cache_error()),
        }
        let db = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(|_| cache_error())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .map_err(|_| cache_error())?;
        }
        db.busy_timeout(Duration::from_millis(500))
            .map_err(|_| cache_error())?;
        db.execute_batch(&format!(
            "PRAGMA page_size=4096; PRAGMA max_page_count={MAX_DATABASE_PAGES};
            PRAGMA journal_mode=MEMORY; PRAGMA temp_store=MEMORY; PRAGMA secure_delete=ON;
            CREATE TABLE IF NOT EXISTS discovery (
                scope TEXT NOT NULL, token_hash TEXT NOT NULL,
                fetched_at INTEGER NOT NULL, expires_at INTEGER NOT NULL,
                last_used INTEGER NOT NULL, snapshot TEXT NOT NULL,
                PRIMARY KEY(scope, token_hash));
            CREATE INDEX IF NOT EXISTS discovery_lru ON discovery(last_used);"
        ))
        .map_err(|_| cache_error())?;
        let cache = Self(Arc::new(Inner {
            db: Mutex::new(db),
            idle_ttl: idle_ttl.as_secs().max(1),
            max_age: max_age.as_secs().max(1),
            max_entries,
        }));
        {
            let db = cache.0.db.lock().map_err(|_| cache_error())?;
            cache.prune(&db, now())?;
        }
        Ok(cache)
    }

    pub(crate) fn fresh_until(&self, fetched_at: u64) -> u64 {
        fetched_at.saturating_add(self.0.max_age)
    }

    pub async fn load(&self, scope: &str, token_hash: &str) -> Result<Option<Snapshot>> {
        let (cache, scope, token_hash) = (self.clone(), scope.to_owned(), token_hash.to_owned());
        tokio::task::spawn_blocking(move || cache.load_at(&scope, &token_hash, now()))
            .await
            .map_err(|_| cache_error())?
    }

    pub async fn store(&self, scope: &str, token_hash: &str, snapshot: &Snapshot) -> Result<()> {
        let (cache, scope, token_hash, snapshot) = (
            self.clone(),
            scope.to_owned(),
            token_hash.to_owned(),
            snapshot.clone(),
        );
        tokio::task::spawn_blocking(move || cache.store_at(&scope, &token_hash, &snapshot, now()))
            .await
            .map_err(|_| cache_error())?
    }

    pub async fn invalidate(&self, scope: &str, token_hash: &str) -> Result<()> {
        let (cache, scope, token_hash) = (self.clone(), scope.to_owned(), token_hash.to_owned());
        tokio::task::spawn_blocking(move || {
            cache
                .0
                .db
                .lock()
                .map_err(|_| cache_error())?
                .execute(
                    "DELETE FROM discovery WHERE scope=?1 AND token_hash=?2",
                    params![scope, token_hash],
                )
                .map_err(|_| cache_error())?;
            Ok(())
        })
        .await
        .map_err(|_| cache_error())?
    }

    fn load_at(&self, scope: &str, token_hash: &str, time: u64) -> Result<Option<Snapshot>> {
        let db = self.0.db.lock().map_err(|_| cache_error())?;
        self.prune(&db, time)?;
        let raw: Option<String> = db
            .query_row(
                "SELECT snapshot FROM discovery WHERE scope=?1 AND token_hash=?2",
                params![scope, token_hash],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| cache_error())?;
        let Some(raw) = raw else { return Ok(None) };
        if raw.len() > MAX_SNAPSHOT_BYTES {
            return Err(cache_error());
        }
        let snapshot = serde_json::from_str(&raw).map_err(|_| cache_error())?;
        db.execute(
            "UPDATE discovery SET last_used=?3 WHERE scope=?1 AND token_hash=?2",
            params![scope, token_hash, sql_time(time)?],
        )
        .map_err(|_| cache_error())?;
        Ok(Some(snapshot))
    }

    fn store_at(
        &self,
        scope: &str,
        token_hash: &str,
        snapshot: &Snapshot,
        time: u64,
    ) -> Result<()> {
        if scope.len() != 64
            || token_hash.len() != 64
            || !scope
                .bytes()
                .chain(token_hash.bytes())
                .all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error::invalid(
                "Cache identities must be full SHA-256 hashes",
            ));
        }
        let mut snapshot = snapshot.clone();
        snapshot.expires_at = snapshot
            .expires_at
            .min(snapshot.fetched_at.saturating_add(self.0.max_age));
        if snapshot.expires_at <= time {
            return Ok(());
        }
        let raw = serde_json::to_string(&snapshot).map_err(|_| cache_error())?;
        if raw.len() > MAX_SNAPSHOT_BYTES {
            return Err(Error::new(
                502,
                "discovery_too_large",
                "Discovery metadata exceeds the cache limit",
            ));
        }
        let mut db = self.0.db.lock().map_err(|_| cache_error())?;
        let tx = db.transaction().map_err(|_| cache_error())?;
        self.prune(&tx, time)?;
        tx.execute(
            "DELETE FROM discovery WHERE scope=?1 AND token_hash=?2",
            params![scope, token_hash],
        )
        .map_err(|_| cache_error())?;
        // Bound row count before insertion, even when replacing an entry.
        tx.execute("DELETE FROM discovery WHERE rowid IN (SELECT rowid FROM discovery ORDER BY last_used DESC, rowid DESC LIMIT -1 OFFSET ?1)",
            [self.0.max_entries.saturating_sub(1) as i64]).map_err(|_| cache_error())?;
        tx.execute("INSERT INTO discovery(scope,token_hash,fetched_at,expires_at,last_used,snapshot) VALUES(?1,?2,?3,?4,?5,?6)",
            params![scope, token_hash, sql_time(snapshot.fetched_at)?, sql_time(snapshot.expires_at)?, sql_time(time)?, raw]).map_err(|_| cache_error())?;
        tx.commit().map_err(|_| cache_error())?;
        Ok(())
    }

    fn prune(&self, db: &Connection, time: u64) -> Result<()> {
        db.execute(
            "DELETE FROM discovery WHERE expires_at<=?1 OR fetched_at<=?2 OR last_used<=?3",
            params![
                sql_time(time)?,
                sql_time(time.saturating_sub(self.0.max_age))?,
                sql_time(time.saturating_sub(self.0.idle_ttl))?
            ],
        )
        .map_err(|_| cache_error())?;
        db.execute("DELETE FROM discovery WHERE rowid IN (SELECT rowid FROM discovery ORDER BY last_used DESC, rowid DESC LIMIT -1 OFFSET ?1)",
            [self.0.max_entries as i64]).map_err(|_| cache_error())?;
        Ok(())
    }
}

pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn cache_error() -> Error {
    Error::new(
        500,
        "cache_unavailable",
        "Discovery cache could not be read or written; remove the disposable cache.db while the service is stopped",
    )
}
fn reject_symlinks(path: &Path) -> Result<()> {
    // System parents such as macOS /var may be symlinks. Reject the actual
    // DB target; SQLite's NOFOLLOW repeats this check when it opens it.
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(Error::invalid(
                "The discovery cache path must not contain symbolic links",
            ));
        }
        Ok(_) => (),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(_) => return Err(cache_error()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn sample(fetched_at: u64, expires_at: u64) -> Snapshot {
        Snapshot {
            tools: vec!["bao_request".into()],
            capabilities: json!({}),
            mounts: json!({}),
            complete: false,
            warnings: vec![],
            fetched_at,
            expires_at,
        }
    }
    fn cache(capacity: usize) -> (tempfile::TempDir, Cache) {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::open(
            &dir.path().join("cache.db"),
            Duration::from_secs(20),
            Duration::from_secs(60),
            capacity,
        )
        .unwrap();
        (dir, cache)
    }
    #[test]
    fn age_is_fixed_while_idle_lifetime_slides_and_scope_isolated() {
        let (_dir, cache) = cache(4);
        let hash = "a".repeat(64);
        let scope = "b".repeat(64);
        cache
            .store_at(&scope, &hash, &sample(100, 1000), 100)
            .unwrap();
        assert!(
            cache
                .load_at(&"c".repeat(64), &hash, 101)
                .unwrap()
                .is_none()
        );
        for time in [110, 120, 130, 140, 150] {
            assert!(cache.load_at(&scope, &hash, time).unwrap().is_some());
        }
        assert!(cache.load_at(&scope, &hash, 160).unwrap().is_none());
        cache
            .store_at(&scope, &hash, &sample(200, 1000), 200)
            .unwrap();
        assert!(cache.load_at(&scope, &hash, 220).unwrap().is_none());
    }
    #[test]
    fn token_expiry_and_row_capacity_are_enforced() {
        let (_dir, cache) = cache(2);
        let scope = "a".repeat(64);
        for (i, time) in [("b", 100), ("c", 101), ("d", 102)] {
            cache
                .store_at(&scope, &i.repeat(64), &sample(time, time + 5), time)
                .unwrap();
        }
        assert!(
            cache
                .load_at(&scope, &"b".repeat(64), 103)
                .unwrap()
                .is_none()
        );
        assert!(
            cache
                .load_at(&scope, &"d".repeat(64), 106)
                .unwrap()
                .is_some()
        );
        assert!(
            cache
                .load_at(&scope, &"d".repeat(64), 107)
                .unwrap()
                .is_none()
        );
    }
    #[cfg(unix)]
    #[test]
    fn private_file_and_symlink_rejection() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let (dir, _cache) = cache(2);
        let path = dir.path().join("cache.db");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let linked = dir.path().join("link.db");
        symlink(path, &linked).unwrap();
        assert!(Cache::open(&linked, Duration::from_secs(1), Duration::from_secs(1), 1).is_err());
    }
}

fn sql_time(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| cache_error())
}
