use std::{fs, path::Path};

use anyhow::{Context, Result};

use crate::{
    cli::ApplicationKind,
    filesystem::{ensure_destination_available, package_name, reserve_destination},
    templates::{template_files, STANDARD_DIRECTORIES},
};

pub(crate) fn create_project(path: &Path, kind: ApplicationKind) -> Result<()> {
    ensure_destination_available(path)?;
    let package_name = package_name(path)?;
    let files = template_files(path, &package_name, kind)?;
    reserve_destination(path)?;

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

    println!(
        "Created {} Scafra application in {}",
        kind.as_str(),
        path.display()
    );
    Ok(())
}
