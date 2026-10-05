//! Coder workspaces saved as devices. Herdr's `endpoints.json` has a fixed SSH
//! schema owned upstream, so the GUI keeps these in its own state directory.
//! Reads and writes are bounded filesystem I/O for background workers only.

use super::{Error, Result, names};
use herdr_client::ConnectTarget;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

const FILE: &str = "coder-workspaces.json";
const LIMIT: u64 = 256 * 1024;
const MAX_WORKSPACES: usize = 64;
const LABEL_LIMIT: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SavedWorkspace {
    /// Coder's workspace ID; stable across renames of the label.
    pub(crate) id: String,
    pub(crate) label: String,
    /// The deployment root the workspace belongs to, as in `Settings::base`.
    pub(crate) deployment: String,
    pub(crate) name: String,
    pub(crate) session: String,
    pub(crate) enabled: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    workspaces: Vec<SavedWorkspace>,
}

impl SavedWorkspace {
    pub(crate) fn endpoint_id(&self) -> String {
        format!("coder:{}", self.id)
    }

    pub(crate) fn target(&self) -> ConnectTarget {
        ConnectTarget::Coder {
            deployment: self.deployment.clone(),
            workspace: self.name.clone(),
            session: self.session.clone(),
        }
    }

    fn valid(&self) -> bool {
        !self.id.is_empty()
            && self.id.len() <= 128
            && self
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && !self.label.trim().is_empty()
            && self.label.len() <= LABEL_LIMIT
            && !self.label.chars().any(char::is_control)
            && !self.deployment.is_empty()
            && self.deployment.len() <= 2048
            && names::valid(&self.name)
            && herdr_client::session_socket(Path::new(""), &self.session).is_ok()
    }
}

fn path() -> Result<PathBuf> {
    crate::preferences::state_dir()
        .map(|dir| dir.join(FILE))
        .ok_or(Error::Field("state directory"))
}

fn io(path: &Path) -> impl FnOnce(io::Error) -> Error + '_ {
    move |source| Error::Catalog {
        path: path.to_owned(),
        source,
    }
}

fn read(path: &Path) -> Result<Vec<SavedWorkspace>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io(path)(error)),
    };
    let mut bytes = Vec::new();
    file.take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(io(path))?;
    if bytes.len() as u64 > LIMIT {
        return Err(Error::Field("coder-workspaces.json size"));
    }
    let document: Document = serde_json::from_slice(&bytes).map_err(Error::json)?;
    if document.version != 1
        || document.workspaces.len() > MAX_WORKSPACES
        || !document.workspaces.iter().all(SavedWorkspace::valid)
    {
        return Err(Error::Field("coder-workspaces.json"));
    }
    Ok(document.workspaces)
}

fn write(path: &Path, workspaces: Vec<SavedWorkspace>) -> Result<()> {
    if workspaces.len() > MAX_WORKSPACES || !workspaces.iter().all(SavedWorkspace::valid) {
        return Err(Error::Field("Coder workspace"));
    }
    let parent = path.parent().ok_or(Error::Field("state directory"))?;
    fs::create_dir_all(parent).map_err(io(parent))?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(io(parent))?;
    serde_json::to_writer_pretty(
        &mut file,
        &Document {
            version: 1,
            workspaces,
        },
    )
    .map_err(Error::json)?;
    file.write_all(b"\n").map_err(io(path))?;
    file.as_file().sync_all().map_err(io(path))?;
    file.persist(path).map_err(|error| io(path)(error.error))?;
    Ok(())
}

// Windows each run their own workers; a read-modify-write must not interleave.
fn transaction<T>(work: impl FnOnce(&Path) -> Result<T>) -> Result<T> {
    static ACCESS: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = ACCESS.lock().unwrap_or_else(|error| error.into_inner());
    work(&path()?)
}

pub(crate) fn load() -> Result<Vec<SavedWorkspace>> {
    transaction(read)
}

/// Add `workspace`, or replace the entry with its ID.
pub(crate) fn save(workspace: SavedWorkspace) -> Result<()> {
    transaction(|path| save_in(path, workspace))
}

pub(crate) fn remove(id: &str) -> Result<()> {
    transaction(|path| {
        let mut workspaces = read(path)?;
        workspaces.retain(|saved| saved.id != id);
        write(path, workspaces)
    })
}

fn save_in(path: &Path, workspace: SavedWorkspace) -> Result<()> {
    let mut workspaces = read(path)?;
    match workspaces.iter_mut().find(|saved| saved.id == workspace.id) {
        Some(saved) => *saved = workspace,
        None => workspaces.push(workspace),
    }
    write(path, workspaces)
}

#[cfg(test)]
mod tests;
