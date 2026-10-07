use std::{
    fs,
    path::{Path, PathBuf},
    process::{self, Child, Command},
    thread,
    time::{Duration, SystemTime},
};

use anyhow::{bail, Context, Result};

pub(crate) fn reserve_destination(path: &Path) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("could not create destination parent {}", parent.display()))?;
    }

    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            bail!("refusing to overwrite existing path {}", path.display())
        }
        Err(error) => {
            Err(error).with_context(|| format!("could not create destination {}", path.display()))
        }
    }
}

pub(crate) fn ensure_destination_available(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => bail!("refusing to overwrite existing path {}", path.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("could not inspect destination {}", path.display()))
        }
    }
}

pub(crate) fn package_name(path: &Path) -> Result<String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("project path must end in a valid UTF-8 directory name")?;
    Ok(normalize_package_name(name))
}

pub(crate) fn normalize_package_name(name: &str) -> String {
    let mut normalized = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if normalized
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_digit())
    {
        normalized.insert(0, '_');
    }
    normalized
}

pub(crate) fn local_dependency(project: &Path, package: &str) -> Result<Option<String>> {
    let local_crate = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../")
        .join(package);
    if !local_crate.exists() {
        return Ok(None);
    }
    let project = absolute_path(project)?;
    let local_crate = fs::canonicalize(&local_crate).with_context(|| {
        format!(
            "could not resolve local dependency {}",
            local_crate.display()
        )
    })?;
    let relative = relative_path(&project, &local_crate)?;
    let relative = manifest_path(&relative)?;
    let published_package = match package {
        "ember" => "ember-framework",
        other => other,
    };
    Ok(Some(format!(
        "{package} = {{ package = \"{published_package}\", path = \"{relative}\", version = \"0.1.0\" }}"
    )))
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::Normal(component) => normalized.push(component),
            std::path::Component::Prefix(component) => normalized.push(component.as_os_str()),
            std::path::Component::RootDir => normalized.push(component.as_os_str()),
        }
    }
    Ok(normalized)
}

fn relative_path(from: &Path, to: &Path) -> Result<PathBuf> {
    if path_prefix(from) != path_prefix(to) || has_root(from) != has_root(to) {
        bail!(
            "cannot express local dependency path from {} to {}",
            from.display(),
            to.display()
        );
    }

    let from = from.components().collect::<Vec<_>>();
    let to = to.components().collect::<Vec<_>>();
    let common = from
        .iter()
        .zip(&to)
        .take_while(|(left, right)| left == right)
        .count();
    let mut relative = PathBuf::new();
    for _ in common..from.len() {
        relative.push("..");
    }
    for component in &to[common..] {
        relative.push(component.as_os_str());
    }
    Ok(relative)
}

fn path_prefix(path: &Path) -> Option<&std::ffi::OsStr> {
    path.components().find_map(|component| match component {
        std::path::Component::Prefix(prefix) => Some(prefix.as_os_str()),
        _ => None,
    })
}

fn has_root(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component, std::path::Component::RootDir))
}

pub(crate) fn manifest_path(path: &Path) -> Result<String> {
    let mut components = Vec::new();
    for component in path.components() {
        let component = match component {
            std::path::Component::CurDir => ".",
            std::path::Component::ParentDir => "..",
            std::path::Component::Normal(component) => component
                .to_str()
                .context("local dependency path contains non-UTF-8 text")?,
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                bail!("local dependency path must be relative")
            }
        };
        components.push(escape_toml_basic_string(component));
    }

    if components.is_empty() {
        bail!("local dependency path must not be empty");
    }
    Ok(components.join("/"))
}

pub(crate) fn escape_toml_basic_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\u{08}' => escaped.push_str("\\b"),
            '\t' => escaped.push_str("\\t"),
            '\n' => escaped.push_str("\\n"),
            '\u{0C}' => escaped.push_str("\\f"),
            '\r' => escaped.push_str("\\r"),
            character if character <= '\u{1F}' || character == '\u{7F}' => {
                use std::fmt::Write;
                write!(escaped, "\\u{:04X}", character as u32)
                    .expect("writing to a String cannot fail");
            }
            character => escaped.push(character),
        }
    }
    escaped
}

pub(crate) fn run_cargo(subcommand: &str) -> Result<()> {
    let status = Command::new("cargo")
        .arg(subcommand)
        .status()
        .with_context(|| format!("could not start cargo {subcommand}"))?;
    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

/// Runs the application and restarts it when source or configuration files change.
pub(crate) fn run_dev() -> Result<()> {
    let root = std::env::current_dir().context("could not determine the project directory")?;
    let mut snapshot = project_snapshot(&root)?;
    let mut child = spawn_dev_process()?;
    println!("Ember dev: watching src, Cargo.toml, Cargo.lock, and resources");

    loop {
        thread::sleep(Duration::from_millis(500));
        let current = project_snapshot(&root)?;
        let changed = current != snapshot;
        snapshot = current;

        if changed {
            println!("\nEmber dev: watched file changed, restarting application...\n");
            stop_child(&mut child);
            child = spawn_dev_process()?;
            continue;
        }

        if child.try_wait()?.is_some() {
            println!("\nEmber dev: application stopped; waiting for the next change...");
            loop {
                thread::sleep(Duration::from_millis(500));
                let current = project_snapshot(&root)?;
                if current != snapshot {
                    snapshot = current;
                    println!("\nEmber dev: change detected, restarting application...\n");
                    child = spawn_dev_process()?;
                    break;
                }
            }
        }
    }
}

fn spawn_dev_process() -> Result<Child> {
    Command::new("cargo")
        .arg("run")
        // The CLI is already the development supervisor. This prevents the
        // embedded Ember supervisor from nesting another watcher inside it.
        .env("EMBER_DEV_CHILD", "1")
        .spawn()
        .context("could not start cargo run")
}

fn stop_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn project_snapshot(root: &Path) -> Result<Vec<(PathBuf, Option<SystemTime>, u64)>> {
    let mut files = Vec::new();
    for directory in ["src", "examples", "tests"] {
        let path = root.join(directory);
        if path.exists() {
            collect_snapshot_files(&path, &mut files)?;
        }
    }
    for file in ["Cargo.toml", "Cargo.lock"] {
        let path = root.join(file);
        if path.exists() {
            add_snapshot_file(&path, &mut files)?;
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(files)
}

fn collect_snapshot_files(
    path: &Path,
    files: &mut Vec<(PathBuf, Option<SystemTime>, u64)>,
) -> Result<()> {
    for entry in fs::read_dir(path).with_context(|| format!("could not read {}", path.display()))? {
        let entry = entry?;
        let entry_path = entry.path();
        if entry_path
            .file_name()
            .is_some_and(|name| name == "target" || name == ".git")
        {
            continue;
        }
        if entry.file_type()?.is_dir() {
            collect_snapshot_files(&entry_path, files)?;
        } else if entry.file_type()?.is_file() && is_watched_file(&entry_path) {
            add_snapshot_file(&entry_path, files)?;
        }
    }
    Ok(())
}

fn is_watched_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("rs" | "yml" | "yaml" | "properties")
    )
}

fn add_snapshot_file(
    path: &Path,
    files: &mut Vec<(PathBuf, Option<SystemTime>, u64)>,
) -> Result<()> {
    let metadata = fs::metadata(path)?;
    files.push((path.to_owned(), metadata.modified().ok(), metadata.len()));
    Ok(())
}
