use crate::{Error, Result};
use std::{net::SocketAddr, path::PathBuf, time::Duration};
#[derive(Clone)]
pub struct Config {
    pub addr: String,
    pub namespace: String,
    pub bind: SocketAddr,
    pub ca_file: Option<PathBuf>,
    pub cache_path: PathBuf,
    pub cache_idle_ttl: Duration,
    pub cache_max_age: Duration,
    pub cache_max_entries: usize,
    pub timeout: Duration,
    pub allowed_hosts: Vec<String>,
    pub allowed_origins: Vec<String>,
}
impl Config {
    pub fn from_env() -> Result<Self> {
        Self::from_values(|key| std::env::var(key).ok())
    }
    pub fn from_values(get: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let number = |name: &str, default: u64, max: u64| -> Result<u64> {
            let n = get(name)
                .map(|v| v.parse::<u64>())
                .transpose()
                .map_err(|_| Error::invalid("Numeric configuration must be a positive integer"))?
                .unwrap_or(default);
            if n == 0 || n > max {
                return Err(Error::invalid(
                    "Numeric configuration is outside its permitted range",
                ));
            }
            Ok(n)
        };
        let list = |name: &str| -> Vec<String> {
            get(name)
                .unwrap_or_default()
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        };
        let namespace = get("BAO_NAMESPACE")
            .unwrap_or_else(|| "agent".into())
            .trim_matches('/')
            .to_owned();
        if namespace.is_empty() {
            return Err(Error::invalid(
                "BAO_NAMESPACE must name an existing namespace",
            ));
        }
        let mut hosts = vec![
            "localhost".into(),
            "localhost:*".into(),
            "127.0.0.1".into(),
            "127.0.0.1:*".into(),
            "[::1]".into(),
            "[::1]:*".into(),
        ];
        hosts.extend(list("MCP_ALLOWED_HOSTS"));
        let mut origins = vec![
            "http://localhost:*".into(),
            "http://127.0.0.1:*".into(),
            "http://[::1]:*".into(),
        ];
        origins.extend(list("MCP_ALLOWED_ORIGINS"));
        Ok(Self {
            addr: get("BAO_ADDR").ok_or_else(|| Error::invalid("BAO_ADDR is required"))?,
            namespace,
            bind: get("MCP_LISTEN")
                .unwrap_or_else(|| "127.0.0.1:8000".into())
                .parse()
                .map_err(|_| Error::invalid("MCP_LISTEN must be a socket address"))?,
            ca_file: get("BAO_CACERT")
                .filter(|s| !s.is_empty())
                .map(PathBuf::from),
            cache_path: get("CACHE_PATH")
                .unwrap_or_else(|| "cache.db".into())
                .into(),
            cache_idle_ttl: Duration::from_secs(number("CACHE_IDLE_TTL_SECONDS", 3600, 31536000)?),
            cache_max_age: Duration::from_secs(number("CACHE_MAX_AGE_SECONDS", 60, 3600)?),
            cache_max_entries: number("CACHE_MAX_ENTRIES", 1024, 10000)? as usize,
            timeout: Duration::from_secs(number("BAO_TIMEOUT_SECONDS", 30, 300)?),
            allowed_hosts: hosts,
            allowed_origins: origins,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_address_required() {
        let cfg =
            Config::from_values(|k| (k == "BAO_ADDR").then(|| "http://127.0.0.1:8200".into()))
                .unwrap();
        assert_eq!(cfg.namespace, "agent");
        assert_eq!(cfg.cache_path, PathBuf::from("cache.db"));
    }
    #[test]
    fn reject_empty_namespace_and_unbounded_settings() {
        for (key, value) in [
            ("BAO_NAMESPACE", "/"),
            ("CACHE_MAX_ENTRIES", "0"),
            ("CACHE_MAX_AGE_SECONDS", "99999999"),
        ] {
            assert!(
                Config::from_values(|k| if k == "BAO_ADDR" {
                    Some("http://127.0.0.1:8200".into())
                } else if k == key {
                    Some(value.into())
                } else {
                    None
                })
                .is_err()
            );
        }
    }
}
