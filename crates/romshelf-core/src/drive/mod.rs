//! Google Drive seam. The Google implementation arrives in stage 1; the fake is in `romshelf-testkit`.

use crate::Result;
use serde::{Deserialize, Serialize};

pub const FOLDER_MIME: &str = "application/vnd.google-apps.folder";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriveFile {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub parents: Vec<String>,
    pub size: u64,
    pub md5: Option<String>,
    pub trashed: bool,
}

impl DriveFile {
    pub fn is_folder(&self) -> bool {
        self.mime_type == FOLDER_MIME
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub file_id: String,
    /// The file's state after the change; `None` when it was removed permanently.
    pub file: Option<DriveFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangePage {
    pub changes: Vec<Change>,
    pub new_token: String,
}

pub trait DriveClient: Send + Sync {
    fn create_folder(&self, name: &str, parent: &str) -> Result<DriveFile>;
    fn create_file(&self, name: &str, parent: &str, bytes: &[u8]) -> Result<DriveFile>;
    /// Children of a folder, not including trashed files.
    fn list(&self, parent: &str) -> Result<Vec<DriveFile>>;
    fn get(&self, id: &str) -> Result<DriveFile>;
    fn rename(&self, id: &str, new_name: &str) -> Result<DriveFile>;
    fn move_to(&self, id: &str, new_parent: &str) -> Result<DriveFile>;
    /// Move to Drive's trash. Romshelf never deletes permanently.
    fn trash(&self, id: &str) -> Result<()>;
    /// File bytes from `offset` to the end, for resumable downloads.
    fn download(&self, id: &str, offset: u64) -> Result<Vec<u8>>;
    fn start_page_token(&self) -> Result<String>;
    fn changes(&self, token: &str) -> Result<ChangePage>;
}
