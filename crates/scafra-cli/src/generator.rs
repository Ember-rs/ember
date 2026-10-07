use std::{
    fs,
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};

use crate::{
    cli::ApplicationKind,
    filesystem::{ensure_destination_available, package_name},
    templates::{template_files, STANDARD_DIRECTORIES},
};

pub(crate) fn create_project(path: &Path, kind: ApplicationKind) -> Result<()> {
    ensure_destination_available(path)?;
    let package_name = package_name(path)?;
    let files = template_files(path, &package_name, kind)?;
    let staging = StagingDirectory(create_staging_directory(path)?);

    populate_project(&staging.0, &files)?;

    ensure_destination_available(path)?;
    publish_staging_directory(&staging.0, path).with_context(|| {
        format!(
            "could not move completed project from {} to {}",
            staging.0.display(),
            path.display()
        )
    })?;

    println!(
        "Created {} Scafra application in {}",
        kind.as_str(),
        path.display()
    );
    Ok(())
}

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_vendor = "apple",
    target_os = "redox"
))]
pub(crate) fn publish_staging_directory(staging: &Path, destination: &Path) -> std::io::Result<()> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        staging,
        rustix::fs::CWD,
        destination,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(std::io::Error::from)
}

#[cfg(windows)]
pub(crate) fn publish_staging_directory(staging: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileW;

    let staging: Vec<u16> = staging.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();

    // MoveFileW fails if the destination already exists, and both paths are
    // siblings so the directory move stays on the same volume.
    let result = unsafe { MoveFileW(staging.as_ptr(), destination.as_ptr()) };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_vendor = "apple",
    target_os = "redox",
    windows
)))]
pub(crate) fn publish_staging_directory(
    _staging: &Path,
    _destination: &Path,
) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "atomic no-replace directory moves are unsupported on this platform",
    ))
}

fn populate_project(path: &Path, files: &[(&'static str, String)]) -> Result<()> {
    for directory in STANDARD_DIRECTORIES {
        fs::create_dir_all(path.join(directory))
            .with_context(|| format!("could not create {}/{}", path.display(), directory))?;
    }

    for (relative_path, contents) in files {
        let file_path = path.join(relative_path);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("could not create {}", parent.display()))?;
        }
        fs::write(&file_path, contents)
            .with_context(|| format!("could not write {}", file_path.display()))?;
    }

    Ok(())
}

struct StagingDirectory(PathBuf);

impl Drop for StagingDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn create_staging_directory(destination: &Path) -> Result<PathBuf> {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("could not create destination parent {}", parent.display()))?;
    let name = destination
        .file_name()
        .context("project path must end in a directory name")?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    for _ in 0..100 {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let mut staging_name = name.to_os_string();
        staging_name.push(format!(".scafra-tmp-{}-{nonce}-{id}", process::id()));
        let staging = parent.join(staging_name);
        match fs::create_dir(&staging) {
            Ok(()) => return Ok(staging),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("could not create staging directory {}", staging.display())
                })
            }
        }
    }

    anyhow::bail!(
        "could not allocate a unique staging directory beside {}",
        destination.display()
    )
}
