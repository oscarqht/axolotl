//! Session-only native download state. Native engines keep cookies, POST bodies,
//! and blob URLs; bytes are never fetched again by the shell.
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::{
    cell::RefCell,
    collections::HashSet,
    path::{Path, PathBuf},
    rc::Rc,
};

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "windows")]
mod windows;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadStatus {
    Downloading,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize)]
pub struct DownloadItem {
    pub id: String,
    pub filename: String,
    pub path: PathBuf,
    pub received: u64,
    pub total: Option<u64>,
    pub status: DownloadStatus,
    pub error: Option<String>,
}

struct Transfer {
    item: DownloadItem,
    suggested: String,
    temporary: PathBuf,
    native: Option<NativeTransfer>,
}

#[cfg(target_os = "macos")]
type NativeTransfer = mac::Transfer;
#[cfg(target_os = "windows")]
type NativeTransfer = windows::Transfer;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
struct NativeTransfer;

#[derive(Default)]
pub struct DownloadManager {
    transfers: Vec<Transfer>,
    revision: u64,
    seen_revision: u64,
}

impl DownloadManager {
    pub fn busy(&self) -> bool {
        self.transfers
            .iter()
            .any(|t| t.native.is_some() || t.item.status == DownloadStatus::Downloading)
    }
    pub fn downloading_count(&self) -> usize {
        self.transfers
            .iter()
            .filter(|transfer| transfer.item.status == DownloadStatus::Downloading)
            .count()
    }

    pub fn items(&self) -> Vec<DownloadItem> {
        self.transfers.iter().map(|t| t.item.clone()).collect()
    }

    /// Called by the shell timer; native handles remain on the UI thread.
    pub fn poll(&mut self) -> bool {
        let mut reserved: HashSet<PathBuf> = self
            .transfers
            .iter()
            .filter(|t| t.item.status == DownloadStatus::Downloading)
            .map(|t| t.item.path.clone())
            .collect();
        for transfer in &mut self.transfers {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            if transfer.item.status == DownloadStatus::Cancelled {
                if let Some(native) = &transfer.native
                    && native.progress().2.is_some()
                {
                    transfer.native.take();
                    let _ = std::fs::remove_file(&transfer.temporary);
                }
                continue;
            }
            if transfer.item.status != DownloadStatus::Downloading {
                continue;
            }
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            if let Some(native) = &transfer.native {
                let (received, total, finished) = native.progress();
                if received != transfer.item.received || total != transfer.item.total {
                    transfer.item.received = received;
                    transfer.item.total = total;
                    self.revision += 1;
                }
                if let Some(result) = finished {
                    reserved.remove(&transfer.item.path);
                    finish_reserved(transfer, result, &reserved);
                    self.revision += 1;
                }
            }
        }
        let changed = self.revision != self.seen_revision;
        self.seen_revision = self.revision;
        changed
    }

    fn begin(&mut self, suggested: &str) -> Result<(String, PathBuf)> {
        let id = uuid::Uuid::new_v4().to_string();
        let directory = dirs::download_dir();
        let filename = safe_filename(suggested);
        let reserved = self
            .transfers
            .iter()
            .filter(|t| t.item.status == DownloadStatus::Downloading)
            .map(|t| t.item.path.clone())
            .collect();
        let planned = directory
            .as_ref()
            .map(|directory| available_path(directory, &filename, &reserved))
            .transpose();
        let path = planned
            .as_ref()
            .ok()
            .and_then(Clone::clone)
            .unwrap_or_default();
        let temporary = directory
            .as_ref()
            .map(|directory| directory.join(format!(".axolotl-{id}.download")))
            .unwrap_or_default();
        let result = directory
            .context("The system Downloads folder is unavailable")
            .and_then(|directory| {
                std::fs::create_dir_all(directory).context("Create Downloads folder")
            })
            .and_then(|()| planned.map(|_| ()).context("Choose download filename"));
        self.transfers.push(Transfer {
            suggested: filename.clone(),
            item: DownloadItem {
                id: id.clone(),
                filename: path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or(filename),
                path,
                received: 0,
                total: None,
                status: if result.is_ok() {
                    DownloadStatus::Downloading
                } else {
                    DownloadStatus::Failed
                },
                error: result.as_ref().err().map(ToString::to_string),
            },
            temporary: temporary.clone(),
            native: None,
        });
        self.revision += 1;
        result?;
        Ok((id, temporary))
    }

    pub fn cancel(&mut self, id: &str) -> Result<()> {
        let transfer = self
            .transfers
            .iter_mut()
            .find(|t| t.item.id == id)
            .context("Unknown download")?;
        if transfer.item.status != DownloadStatus::Downloading {
            return Ok(());
        }
        // Set state before calling the engine: cancellation may synchronously callback.
        transfer.item.status = DownloadStatus::Cancelled;
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(native) = &transfer.native {
            native.cancel();
        }
        let _ = std::fs::remove_file(&transfer.temporary);
        self.revision += 1;
        Ok(())
    }

    pub fn open(&self, id: &str) -> Result<()> {
        self.file_action(id, false)
    }
    pub fn reveal(&self, id: &str) -> Result<()> {
        self.file_action(id, true)
    }
    fn file_action(&self, id: &str, reveal: bool) -> Result<()> {
        let item = &self
            .transfers
            .iter()
            .find(|t| t.item.id == id)
            .context("Unknown download")?
            .item;
        if item.status != DownloadStatus::Completed {
            bail!("This download has not completed");
        }
        if !item.path.is_file() {
            bail!("The downloaded file has moved or was deleted");
        }
        #[cfg(target_os = "macos")]
        {
            use objc2_app_kit::NSWorkspace;
            use objc2_foundation::{NSArray, NSString, NSURL};
            let url = NSURL::fileURLWithPath_isDirectory(
                &NSString::from_str(&item.path.to_string_lossy()),
                false,
            );
            let workspace = NSWorkspace::sharedWorkspace();
            if reveal {
                workspace.activateFileViewerSelectingURLs(&NSArray::from_slice(&[&*url]));
            } else if !workspace.openURL(&url) {
                bail!("No application could open this downloaded file");
            }
        }
        #[cfg(target_os = "windows")]
        windows::file_action(&item.path, reveal)?;
        Ok(())
    }
}

impl Drop for DownloadManager {
    fn drop(&mut self) {
        for transfer in &mut self.transfers {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            if let Some(native) = transfer.native.take() {
                native.cancel();
            }
            let _ = std::fs::remove_file(&transfer.temporary);
        }
    }
}

#[cfg(any(test, target_os = "windows"))]
fn finish(transfer: &mut Transfer, result: std::result::Result<(), String>) {
    finish_reserved(transfer, result, &HashSet::new());
}

fn finish_reserved(
    transfer: &mut Transfer,
    result: std::result::Result<(), String>,
    reserved: &HashSet<PathBuf>,
) {
    if transfer.item.status != DownloadStatus::Downloading {
        return;
    }
    let result = result.and_then(|()| publish(transfer, reserved).map_err(|e| e.to_string()));
    transfer.item.status = if result.is_ok() {
        DownloadStatus::Completed
    } else {
        DownloadStatus::Failed
    };
    transfer.item.error = result.err();
    transfer.native.take();
    let _ = std::fs::remove_file(&transfer.temporary);
}

fn publish(transfer: &mut Transfer, reserved: &HashSet<PathBuf>) -> Result<()> {
    let directory = transfer
        .item
        .path
        .parent()
        .context("Download path has no parent")?;
    let filename = transfer.suggested.clone();
    loop {
        let candidate = available_path(directory, &filename, reserved)?;
        // Hard-link creation is atomic and fails when a destination exists, so an
        // external writer or another Axolotl process can never be overwritten.
        match std::fs::hard_link(&transfer.temporary, &candidate) {
            Ok(()) => {
                transfer.item.received = std::fs::metadata(&candidate)?.len();
                transfer.item.filename = candidate.file_name().unwrap().to_string_lossy().into();
                transfer.item.path = candidate;
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => match copy_new_file(&transfer.temporary, &candidate) {
                Ok(()) => {
                    transfer.item.received = std::fs::metadata(&candidate)?.len();
                    transfer.item.filename =
                        candidate.file_name().unwrap().to_string_lossy().into();
                    transfer.item.path = candidate;
                    return Ok(());
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error).context("Save downloaded file"),
            },
        }
    }
}

// Some removable or redirected Downloads folders do not support hard links.
// The fallback still creates the destination exclusively and never truncates it.
fn copy_new_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    return windows::copy_new_file(source, destination);
    #[cfg(target_os = "macos")]
    return mac::copy_new_file(source, destination);
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let mut input = std::fs::File::open(source)?;
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)?;
        let result = std::io::copy(&mut input, &mut output).map(|_| ());
        drop(output);
        if result.is_err() {
            let _ = std::fs::remove_file(destination);
        }
        result
    }
}

fn available_path(
    directory: &Path,
    filename: &str,
    reserved: &HashSet<PathBuf>,
) -> std::io::Result<PathBuf> {
    let name = Path::new(filename);
    let stem = name.file_stem().unwrap_or_default().to_string_lossy();
    let extension = name
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for suffix in 0.. {
        let path = directory.join(if suffix == 0 {
            filename.into()
        } else {
            format!("{stem} ({suffix}){extension}")
        });
        let occupied = match std::fs::symlink_metadata(&path) {
            Ok(_) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error),
        };
        if !occupied && !reserved.contains(&path) {
            return Ok(path);
        }
    }
    unreachable!()
}

fn safe_filename(suggested: &str) -> String {
    let name = suggested.rsplit(['/', '\\']).next().unwrap_or_default();
    let mut name: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    name = name.trim().trim_matches('.').trim_end().to_string();
    if name.is_empty() {
        name = "download".into();
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if matches!(
        stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    ) {
        name.insert(0, '_');
    }
    // Leave room for a numbered suffix under the common 255-byte component limit.
    while name.len() > 220 {
        name.pop();
    }
    name
}

pub fn install(view: &wry::WebView, manager: Rc<RefCell<DownloadManager>>) -> Result<()> {
    #[cfg(target_os = "macos")]
    return mac::install(view, manager);
    #[cfg(target_os = "windows")]
    return windows::install(view, manager);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (view, manager);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
