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
    let staging = StagingDirectory(Some(create_staging_directory(path)?));

    populate_project(staging.path(), &files)?;

    ensure_destination_available(path)?;
    publish_staging_directory(staging.path(), path).with_context(|| {
        format!(
            "could not move completed project from {} to {}",
            staging.path().display(),
            path.display()
        )
    })?;
    staging.persist();

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
pub(crate) fn publish_staging_directory(staging: &Path, destination: &Path) -> std::io::Result<()> {
    // Keep generation available on less common targets. On these platforms
    // rename replacement behavior follows the standard library/OS semantics.
    fs::rename(staging, destination)
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

struct StagingDirectory(Option<PathBuf>);

impl StagingDirectory {
    fn path(&self) -> &Path {
        self.0
            .as_deref()
            .expect("staging path is present until persist")
    }

    fn persist(mut self) {
        self.0.take();
    }
}

impl Drop for StagingDirectory {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = fs::remove_dir_all(path);
        }
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

#[cfg(test)]
mod tests {
    use super::StagingDirectory;
    use std::{
        fs,
        path::PathBuf,
        process,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn persisted_staging_guard_does_not_remove_recreated_path() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("scafra-staging-persist-{}-{nonce}", process::id()));
        let staging = root.join("staging");
        let published = root.join("published");
        fs::create_dir_all(&staging).unwrap();
        let guard = StagingDirectory(Some(PathBuf::from(&staging)));

        fs::rename(&staging, &published).unwrap();
        fs::create_dir(&staging).unwrap();
        guard.persist();

        assert!(staging.is_dir());
        fs::remove_dir_all(root).unwrap();
    }
}
