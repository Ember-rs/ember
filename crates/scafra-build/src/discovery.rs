use std::{fs, io, path::Path};

use crate::model::{ModuleNode, SourceFile};
pub(crate) fn collect_files(
    directory: &Path,
    module_path: &mut Vec<String>,
    node: &mut ModuleNode,
) -> io::Result<()> {
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, io::Error>>()?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if is_valid_module_name(&name) {
                collect_files(
                    &path,
                    &mut module_path_with(&*module_path, &name),
                    node.children.entry(name).or_default(),
                )?;
            }
            continue;
        }
        if !file_type.is_file() || path.extension().and_then(|value| value.to_str()) != Some("rs") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if matches!(stem, "main" | "lib" | "mod") {
            continue;
        }
        let module_name = sanitize_ident(stem);
        if node.files.insert(module_name.clone(), path).is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "duplicate Scafra module `{module_name}` in {}",
                    directory.display()
                ),
            ));
        }
    }
    Ok(())
}

pub(crate) fn collect_source_files(
    directory: &Path,
    module_path: &mut Vec<String>,
    manifest_dir: &Path,
    files: &mut Vec<SourceFile>,
) -> io::Result<()> {
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, io::Error>>()?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if is_valid_module_name(&name) {
                module_path.push(name);
                collect_source_files(&path, module_path, manifest_dir, files)?;
                module_path.pop();
            }
            continue;
        }
        if !file_type.is_file() || path.extension().and_then(|value| value.to_str()) != Some("rs") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if matches!(stem, "main" | "lib" | "mod") {
            continue;
        }
        let source = fs::read_to_string(&path)?;
        let syntax = syn::parse_file(&source).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("could not parse Scafra source {}: {error}", path.display()),
            )
        })?;
        let source_label = path
            .strip_prefix(manifest_dir)
            .unwrap_or(&path)
            .display()
            .to_string();
        files.push(SourceFile {
            module_path: module_path
                .iter()
                .map(|segment| sanitize_ident(segment))
                .chain(std::iter::once(sanitize_ident(stem)))
                .collect(),
            source_label,
            syntax,
        });
    }
    Ok(())
}

fn module_path_with(path: &[String], child: &str) -> Vec<String> {
    let mut result = path.to_owned();
    result.push(child.to_owned());
    result
}

pub(crate) fn is_valid_module_name(name: &str) -> bool {
    !name.starts_with('.') && name != "target"
}

pub(crate) fn sanitize_ident(value: &str) -> String {
    let mut result = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if result.is_empty() {
        result.push_str("module");
    }
    if result
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_digit())
    {
        result.insert(0, '_');
    }
    result
}
