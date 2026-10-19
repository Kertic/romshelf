//! OAuth token storage seam: the system keyring, falling back to a private file when no
//! keyring service is running (Game Mode may have none).

use crate::{Error, Result};
use std::path::PathBuf;

pub trait TokenStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>>;
    fn set(&self, key: &str, value: &str) -> Result<()>;
    fn delete(&self, key: &str) -> Result<()>;
}

/// JSON map in a file readable only by the user (mode 0600 on Unix).
pub struct FileTokenStore {
    pub path: PathBuf,
}

type Map = serde_json::Map<String, serde_json::Value>;

impl FileTokenStore {
    fn load(&self) -> Result<Map> {
        match std::fs::read(&self.path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
            Err(e) => Err(e.into()),
        }
    }

    fn save(&self, map: &Map) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec(map)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(tmp, &self.path)?;
        Ok(())
    }
}

impl TokenStore for FileTokenStore {
    fn get(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .load()?
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::to_string))
    }

    fn set(&self, key: &str, value: &str) -> Result<()> {
        let mut map = self.load()?;
        map.insert(key.into(), value.into());
        self.save(&map)
    }

    fn delete(&self, key: &str) -> Result<()> {
        let mut map = self.load()?;
        if map.remove(key).is_some() {
            self.save(&map)?;
        }
        Ok(())
    }
}

/// The system keyring (Secret Service on Linux).
pub struct KeyringTokenStore {
    pub service: String,
}

impl KeyringTokenStore {
    fn entry(&self, key: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(&self.service, key).map_err(|e| Error::External(e.to_string()))
    }
}

impl TokenStore for KeyringTokenStore {
    fn get(&self, key: &str) -> Result<Option<String>> {
        match self.entry(key)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(Error::External(e.to_string())),
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<()> {
        self.entry(key)?
            .set_password(value)
            .map_err(|e| Error::External(e.to_string()))
    }

    fn delete(&self, key: &str) -> Result<()> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(Error::External(e.to_string())),
        }
    }
}

/// Tries `primary` and silently uses `fallback` when it fails. A value lives in one place:
/// writing to the primary removes any copy from the fallback.
pub struct FallbackTokenStore<P, F> {
    pub primary: P,
    pub fallback: F,
}

impl<P: TokenStore, F: TokenStore> TokenStore for FallbackTokenStore<P, F> {
    fn get(&self, key: &str) -> Result<Option<String>> {
        match self.primary.get(key) {
            Ok(Some(v)) => Ok(Some(v)),
            Ok(None) | Err(_) => self.fallback.get(key),
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<()> {
        match self.primary.set(key, value) {
            Ok(()) => {
                let _ = self.fallback.delete(key);
                Ok(())
            }
            Err(_) => self.fallback.set(key, value),
        }
    }

    fn delete(&self, key: &str) -> Result<()> {
        let _ = self.primary.delete(key);
        self.fallback.delete(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A keyring that is never available.
    struct Broken;

    impl TokenStore for Broken {
        fn get(&self, _: &str) -> Result<Option<String>> {
            Err(Error::External("no secret service".into()))
        }
        fn set(&self, _: &str, _: &str) -> Result<()> {
            Err(Error::External("no secret service".into()))
        }
        fn delete(&self, _: &str) -> Result<()> {
            Err(Error::External("no secret service".into()))
        }
    }

    #[test]
    fn unavailable_keyring_falls_back_to_the_file_silently() {
        let dir = tempfile::tempdir().unwrap();
        let store = FallbackTokenStore {
            primary: Broken,
            fallback: FileTokenStore {
                path: dir.path().join("token.json"),
            },
        };
        store.set("k", "v").unwrap();
        assert_eq!(store.get("k").unwrap().as_deref(), Some("v"));
        assert!(dir.path().join("token.json").exists());
        store.delete("k").unwrap();
        assert_eq!(store.get("k").unwrap(), None);
    }

    #[test]
    fn working_keyring_takes_the_value_and_clears_the_file_copy() {
        let dir = tempfile::tempdir().unwrap();
        let file = FileTokenStore {
            path: dir.path().join("token.json"),
        };
        file.set("k", "stale").unwrap();
        let store = FallbackTokenStore {
            primary: FileTokenStore {
                path: dir.path().join("keyring.json"),
            },
            fallback: file,
        };
        assert_eq!(store.get("k").unwrap().as_deref(), Some("stale"));
        store.set("k", "fresh").unwrap();
        assert_eq!(store.get("k").unwrap().as_deref(), Some("fresh"));
        assert_eq!(store.fallback.get("k").unwrap(), None);
    }

    #[test]
    fn file_store_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileTokenStore {
            path: dir.path().join("cfg/token.json"),
        };
        assert_eq!(store.get("refresh").unwrap(), None);
        store.set("refresh", "abc").unwrap();
        assert_eq!(store.get("refresh").unwrap().as_deref(), Some("abc"));
        store.delete("refresh").unwrap();
        assert_eq!(store.get("refresh").unwrap(), None);
    }

    #[cfg(unix)]
    #[test]
    fn file_store_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let store = FileTokenStore {
            path: dir.path().join("token.json"),
        };
        store.set("k", "v").unwrap();
        let mode = std::fs::metadata(&store.path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
