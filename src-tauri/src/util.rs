use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds since the Unix epoch. Used for every timestamp crossing the
/// IPC boundary so the frontend can hand it straight to `Date`.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Task ids only need to be unique within one install, and to sort roughly by
/// creation time. Timestamp + counter is enough and avoids a uuid dependency.
pub fn new_id(prefix: &str) -> String {
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{:x}-{:x}", now_ms(), seq)
}

/// Stable short hash, used for cache file names.
pub fn hash_key(input: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(input.as_bytes());
    hex::encode(&digest[..12])
}
