use romshelf_core::drive::{Account, Change, ChangePage, DriveClient, DriveFile, FOLDER_MIME};
use romshelf_core::{Error, Result};
use std::collections::BTreeMap;
use std::sync::Mutex;

pub(crate) fn md5_hex(bytes: &[u8]) -> String {
    use md5::{Digest, Md5};
    Md5::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The account the fake Drive reports as signed in.
pub const FAKE_ACCOUNT: &str = "player@example.com";

/// In-memory Drive: a folder tree with a change feed. The root folder id is `root`.
pub struct FakeDrive {
    state: Mutex<State>,
}

struct State {
    files: BTreeMap<String, DriveFile>,
    content: BTreeMap<String, Vec<u8>>,
    /// Change log; a page token is an index into it.
    log: Vec<String>,
    next_id: u64,
    /// Offsets asked for by `download_to`, per file.
    offsets: BTreeMap<String, Vec<u64>>,
    /// The next `download_to` stops with a network error after this many bytes.
    fail_after: Option<u64>,
}

impl Default for FakeDrive {
    fn default() -> FakeDrive {
        FakeDrive {
            state: Mutex::new(State {
                files: BTreeMap::new(),
                content: BTreeMap::new(),
                log: Vec::new(),
                next_id: 1,
                offsets: BTreeMap::new(),
                fail_after: None,
            }),
        }
    }
}

impl State {
    fn insert(
        &mut self,
        name: &str,
        parent: &str,
        mime: &str,
        bytes: Option<&[u8]>,
    ) -> Result<DriveFile> {
        if parent != "root" && !self.files.get(parent).is_some_and(DriveFile::is_folder) {
            return Err(Error::NotFound(format!("folder {parent}")));
        }
        let id = format!("f{:04}", self.next_id);
        self.next_id += 1;
        let file = DriveFile {
            id: id.clone(),
            name: name.into(),
            mime_type: mime.into(),
            parents: vec![parent.into()],
            size: bytes.map_or(0, |b| b.len() as u64),
            md5: bytes
                .filter(|_| !mime.starts_with("application/vnd.google-apps."))
                .map(md5_hex),
            trashed: false,
        };
        if let Some(b) = bytes {
            self.content.insert(id.clone(), b.to_vec());
        }
        self.files.insert(id.clone(), file.clone());
        self.log.push(id);
        Ok(file)
    }

    fn update(&mut self, id: &str, f: impl FnOnce(&mut DriveFile)) -> Result<DriveFile> {
        let file = self
            .files
            .get_mut(id)
            .ok_or_else(|| Error::NotFound(format!("file {id}")))?;
        f(file);
        let file = file.clone();
        self.log.push(id.into());
        Ok(file)
    }
}

impl FakeDrive {
    /// Every file, including trashed ones. For assertions.
    pub fn all_files(&self) -> Vec<DriveFile> {
        self.state.lock().unwrap().files.values().cloned().collect()
    }

    /// A file with a chosen id, as if uploaded elsewhere (collision tests, fixtures).
    pub fn create_file_with_id(
        &self,
        id: &str,
        name: &str,
        parent: &str,
        bytes: &[u8],
    ) -> Result<DriveFile> {
        let mut s = self.state.lock().unwrap();
        let mut file = s.insert(name, parent, "application/octet-stream", Some(bytes))?;
        let generated = std::mem::replace(&mut file.id, id.into());
        s.files.remove(&generated);
        if let Some(c) = s.content.remove(&generated) {
            s.content.insert(id.into(), c);
        }
        s.files.insert(id.into(), file.clone());
        if let Some(last) = s.log.last_mut() {
            *last = id.into();
        }
        Ok(file)
    }

    /// A file of another type, such as a Google Doc or a Drive shortcut.
    pub fn create_file_with_mime(&self, name: &str, parent: &str, mime: &str) -> Result<DriveFile> {
        self.state
            .lock()
            .unwrap()
            .insert(name, parent, mime, Some(b""))
    }

    /// Makes Drive report a different checksum for a file (checksum-mismatch tests).
    pub fn set_md5(&self, id: &str, md5: &str) {
        let _ = self
            .state
            .lock()
            .unwrap()
            .update(id, |f| f.md5 = Some(md5.into()));
    }

    /// The next `download_to` sends this many bytes, then fails like a dropped connection.
    pub fn fail_next_download_after(&self, bytes: u64) {
        self.state.lock().unwrap().fail_after = Some(bytes);
    }

    /// The offsets `download_to` was called with for a file, in order.
    pub fn download_offsets(&self, id: &str) -> Vec<u64> {
        self.state
            .lock()
            .unwrap()
            .offsets
            .get(id)
            .cloned()
            .unwrap_or_default()
    }

    /// Restores a file from Drive's trash.
    pub fn untrash(&self, id: &str) -> Result<()> {
        self.state
            .lock()
            .unwrap()
            .update(id, |f| f.trashed = false)?;
        Ok(())
    }

    /// Simulates a permanent delete done outside Romshelf (for change-feed tests).
    pub fn remove_permanently(&self, id: &str) {
        let mut s = self.state.lock().unwrap();
        s.files.remove(id);
        s.content.remove(id);
        s.log.push(id.into());
    }
}

impl DriveClient for FakeDrive {
    fn about(&self) -> Result<Account> {
        Ok(Account {
            email: FAKE_ACCOUNT.into(),
            name: "Fixture Player".into(),
        })
    }

    fn create_folder(&self, name: &str, parent: &str) -> Result<DriveFile> {
        self.state
            .lock()
            .unwrap()
            .insert(name, parent, FOLDER_MIME, None)
    }

    fn create_file(&self, name: &str, parent: &str, bytes: &[u8]) -> Result<DriveFile> {
        self.state
            .lock()
            .unwrap()
            .insert(name, parent, "application/octet-stream", Some(bytes))
    }

    fn update_content(&self, id: &str, bytes: &[u8]) -> Result<DriveFile> {
        let mut s = self.state.lock().unwrap();
        if s.files.get(id).is_none_or(DriveFile::is_folder) {
            return Err(Error::NotFound(format!("file {id}")));
        }
        s.content.insert(id.into(), bytes.to_vec());
        let md5 = md5_hex(bytes);
        s.update(id, |f| {
            f.size = bytes.len() as u64;
            f.md5 = Some(md5);
        })
    }

    fn list(&self, parent: &str) -> Result<Vec<DriveFile>> {
        let s = self.state.lock().unwrap();
        Ok(s.files
            .values()
            .filter(|f| !f.trashed && f.parents.iter().any(|p| p == parent))
            .cloned()
            .collect())
    }

    fn get(&self, id: &str) -> Result<DriveFile> {
        self.state
            .lock()
            .unwrap()
            .files
            .get(id)
            .cloned()
            .ok_or_else(|| Error::NotFound(format!("file {id}")))
    }

    fn rename(&self, id: &str, new_name: &str) -> Result<DriveFile> {
        self.state
            .lock()
            .unwrap()
            .update(id, |f| f.name = new_name.into())
    }

    fn move_to(&self, id: &str, new_parent: &str) -> Result<DriveFile> {
        let mut s = self.state.lock().unwrap();
        if new_parent != "root" && !s.files.get(new_parent).is_some_and(DriveFile::is_folder) {
            return Err(Error::NotFound(format!("folder {new_parent}")));
        }
        s.update(id, |f| f.parents = vec![new_parent.into()])
    }

    fn trash(&self, id: &str) -> Result<()> {
        self.state
            .lock()
            .unwrap()
            .update(id, |f| f.trashed = true)?;
        Ok(())
    }

    fn download(&self, id: &str, offset: u64) -> Result<Vec<u8>> {
        let s = self.state.lock().unwrap();
        let bytes = s
            .content
            .get(id)
            .ok_or_else(|| Error::NotFound(format!("content {id}")))?;
        Ok(bytes.get(offset as usize..).unwrap_or_default().to_vec())
    }

    fn download_to(&self, id: &str, offset: u64, out: &mut dyn std::io::Write) -> Result<u64> {
        let (bytes, fail_after) = {
            let mut s = self.state.lock().unwrap();
            s.offsets.entry(id.into()).or_default().push(offset);
            let bytes = s
                .content
                .get(id)
                .ok_or_else(|| Error::NotFound(format!("content {id}")))?
                .get(offset as usize..)
                .unwrap_or_default()
                .to_vec();
            (bytes, s.fail_after.take())
        };
        // Small chunks, like a network stream, so progress and cancelling can be seen.
        let limit = fail_after.map_or(bytes.len(), |n| (n as usize).min(bytes.len()));
        for chunk in bytes[..limit].chunks(64 * 1024) {
            out.write_all(chunk)
                .map_err(romshelf_core::http::sink_error)?;
        }
        if fail_after.is_some() {
            return Err(Error::Network("connection reset by peer".into()));
        }
        Ok(limit as u64)
    }

    fn start_page_token(&self) -> Result<String> {
        Ok(self.state.lock().unwrap().log.len().to_string())
    }

    fn changes(&self, token: &str) -> Result<ChangePage> {
        let s = self.state.lock().unwrap();
        let start: usize = token
            .parse()
            .map_err(|_| Error::Invalid(format!("page token {token}")))?;
        let mut seen = Vec::new();
        for id in s.log.iter().skip(start) {
            if !seen.contains(id) {
                seen.push(id.clone());
            }
        }
        Ok(ChangePage {
            changes: seen
                .into_iter()
                .map(|id| Change {
                    file: s.files.get(&id).cloned(),
                    file_id: id,
                })
                .collect(),
            new_token: s.log.len().to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_list_rename_move_trash() {
        let d = FakeDrive::default();
        let roms = d.create_folder("roms", "root").unwrap();
        let gba = d.create_folder("gba", &roms.id).unwrap();
        let snes = d.create_folder("snes", &roms.id).unwrap();
        let f = d.create_file("a.gba", &gba.id, b"1234").unwrap();
        assert_eq!(d.list(&gba.id).unwrap(), vec![f.clone()]);
        assert_eq!(d.rename(&f.id, "b.gba").unwrap().name, "b.gba");
        d.move_to(&f.id, &snes.id).unwrap();
        assert!(d.list(&gba.id).unwrap().is_empty());
        assert_eq!(d.list(&snes.id).unwrap()[0].id, f.id);
        assert_eq!(d.download(&f.id, 2).unwrap(), b"34");
        d.trash(&f.id).unwrap();
        assert!(d.list(&snes.id).unwrap().is_empty());
        assert!(d.get(&f.id).unwrap().trashed);
        assert!(d.create_file("x", "missing", b"").is_err());
        assert!(d.move_to(&f.id, &f.id).is_err(), "cannot move into a file");
    }

    #[test]
    fn change_feed_reports_each_changed_file_once() {
        let d = FakeDrive::default();
        let folder = d.create_folder("roms", "root").unwrap();
        let token = d.start_page_token().unwrap();
        assert!(d.changes(&token).unwrap().changes.is_empty());
        let f = d.create_file("a.gba", &folder.id, b"x").unwrap();
        d.rename(&f.id, "b.gba").unwrap();
        let page = d.changes(&token).unwrap();
        assert_eq!(page.changes.len(), 1);
        assert_eq!(page.changes[0].file.as_ref().unwrap().name, "b.gba");
        d.trash(&f.id).unwrap();
        let page2 = d.changes(&page.new_token).unwrap();
        assert!(page2.changes[0].file.as_ref().unwrap().trashed);
        d.remove_permanently(&f.id);
        let page3 = d.changes(&page2.new_token).unwrap();
        assert_eq!(page3.changes[0].file, None);
        assert!(d.changes(&page3.new_token).unwrap().changes.is_empty());
    }
}
