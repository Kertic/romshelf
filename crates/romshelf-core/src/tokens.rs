//! OAuth token storage seam. Keyring support arrives in stage 1; the file store is the fallback.

use crate::Result;
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

#[cfg(test)]
mod tests {
    use super::*;

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
