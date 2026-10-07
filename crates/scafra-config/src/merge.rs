use crate::errors::ConfigError;
use std::collections::BTreeMap;
pub(crate) const SOURCE_EXTENSIONS: [&str; 3] = ["yaml", "yml", "properties"];

pub(crate) fn validate_profile(profile: Option<&str>) -> Result<(), ConfigError> {
    let Some(profile) = profile else {
        return Ok(());
    };
    if profile.is_empty()
        || profile == "."
        || profile == ".."
        || profile.contains("..")
        || profile.contains('/')
        || profile.contains('\\')
        || profile.chars().any(|character| {
            character.is_control()
                || !(character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'))
        })
    {
        return Err(ConfigError::InvalidProfile);
    }
    Ok(())
}

pub(crate) fn parse_yaml(path: &str, source: &str) -> Result<serde_yaml::Value, ConfigError> {
    serde_yaml::from_str(source).map_err(|error| ConfigError::Parse {
        path: path.to_owned(),
        kind: "YAML",
        location: error
            .location()
            .map(|location| format!(" at line {}", location.line()))
            .unwrap_or_default(),
        message: "invalid syntax",
    })
}

pub(crate) fn merge_properties(
    target: &mut serde_yaml::Value,
    origins: &mut BTreeMap<String, String>,
    path: &str,
    source: &str,
) -> Result<(), ConfigError> {
    for (line_index, line) in source.lines().enumerate() {
        let line_number = line_index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
            continue;
        }

        let Some(separator) = trimmed.find(['=', ':']) else {
            return Err(property_parse_error(
                path,
                line_number,
                "expected key/value separator",
            ));
        };
        let key = trimmed[..separator].trim();
        if key.is_empty() || key.split('.').any(|part| part.trim().is_empty()) {
            return Err(property_parse_error(
                path,
                line_number,
                "invalid dotted key",
            ));
        }
        let value = parse_scalar(trimmed[separator + 1..].trim());
        set_path(target, key, value, path, Some(line_number), origins)?;
    }
    Ok(())
}

fn property_parse_error(path: &str, line: usize, message: &'static str) -> ConfigError {
    ConfigError::Parse {
        path: path.to_owned(),
        kind: "properties",
        location: format!(" at line {line}"),
        message,
    }
}

pub(crate) fn parse_scalar(value: &str) -> serde_yaml::Value {
    match serde_yaml::from_str::<serde_yaml::Value>(value) {
        Ok(value) if !value.is_mapping() && !value.is_sequence() => value,
        _ => serde_yaml::Value::String(value.to_owned()),
    }
}

pub(crate) fn merge_yaml(
    target: &mut serde_yaml::Value,
    source: serde_yaml::Value,
    path: &str,
    source_name: &str,
    origins: &mut BTreeMap<String, String>,
) -> Result<(), ConfigError> {
    match source {
        serde_yaml::Value::Mapping(source_mapping) => {
            if !target.is_mapping() && !target.is_null() {
                return Err(structural_conflict(path, source_name, None));
            }
            if target.is_null() {
                *target = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
            }
            let target_mapping = target
                .as_mapping_mut()
                .expect("a non-null target is a mapping here");
            for (key, value) in source_mapping {
                let child_path = append_path(path, &key);
                if let Some(existing) = target_mapping.get_mut(&key) {
                    merge_yaml(existing, value, &child_path, source_name, origins)?;
                } else {
                    target_mapping.insert(key, value.clone());
                    record_origins(&value, &child_path, source_name, origins);
                }
            }
            Ok(())
        }
        value => {
            if target.is_mapping() && !value.is_null() {
                return Err(structural_conflict(path, source_name, None));
            }
            *target = value;
            if !path.is_empty() {
                origins.insert(path.to_owned(), source_name.to_owned());
            }
            Ok(())
        }
    }
}

pub(crate) fn set_path(
    target: &mut serde_yaml::Value,
    path: &str,
    value: serde_yaml::Value,
    source: &str,
    line: Option<usize>,
    origins: &mut BTreeMap<String, String>,
) -> Result<(), ConfigError> {
    let parts = path.split('.').collect::<Vec<_>>();
    if parts.is_empty() || parts.iter().any(|part| part.is_empty()) {
        return Err(ConfigError::InvalidValue {
            path: path.to_owned(),
            source_kind: source.to_owned(),
        });
    }
    set_path_parts(target, &parts, value, path, source, line, origins)
}

fn set_path_parts(
    target: &mut serde_yaml::Value,
    parts: &[&str],
    value: serde_yaml::Value,
    full_path: &str,
    source: &str,
    line: Option<usize>,
    origins: &mut BTreeMap<String, String>,
) -> Result<(), ConfigError> {
    let mapping = target
        .as_mapping_mut()
        .ok_or_else(|| structural_conflict(full_path, source, line))?;
    let key = serde_yaml::Value::String(parts[0].to_owned());

    if parts.len() == 1 {
        if let Some(existing) = mapping.get_mut(&key) {
            if existing.is_mapping() && !value.is_null() {
                return Err(structural_conflict(full_path, source, line));
            }
            *existing = value;
        } else {
            mapping.insert(key, value);
        }
        origins.insert(full_path.to_owned(), source.to_owned());
        return Ok(());
    }

    let child_path = parts[..2.min(parts.len())].join(".");
    let child = mapping
        .entry(key)
        .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
    if !child.is_mapping() {
        return Err(structural_conflict(&child_path, source, line));
    }
    set_path_parts(child, &parts[1..], value, full_path, source, line, origins)
}

fn record_origins(
    value: &serde_yaml::Value,
    path: &str,
    source: &str,
    origins: &mut BTreeMap<String, String>,
) {
    if let serde_yaml::Value::Mapping(mapping) = value {
        if mapping.is_empty() {
            origins.insert(path.to_owned(), source.to_owned());
        }
        for (key, value) in mapping {
            let child_path = append_path(path, key);
            record_origins(value, &child_path, source, origins);
        }
    } else if !path.is_empty() {
        origins.insert(path.to_owned(), source.to_owned());
    }
}

fn append_path(prefix: &str, key: &serde_yaml::Value) -> String {
    let key = key.as_str().unwrap_or("<non-string-key>");
    if prefix.is_empty() {
        key.to_owned()
    } else {
        format!("{prefix}.{key}")
    }
}

fn structural_conflict(path: &str, source: &str, line: Option<usize>) -> ConfigError {
    ConfigError::StructuralConflict {
        path: path.to_owned(),
        source_kind: source.to_owned(),
        location: line
            .map(|line| format!(" at line {line}"))
            .unwrap_or_default(),
    }
}

pub(crate) fn decode_error(
    error: serde_yaml::Error,
    origins: &BTreeMap<String, String>,
) -> ConfigError {
    let rendered = error.to_string();
    let path = origins
        .keys()
        .filter(|path| rendered.contains(path.as_str()))
        .max_by_key(|path| path.len())
        .cloned()
        .or_else(|| {
            (origins.len() == 1)
                .then(|| origins.keys().next().cloned())
                .flatten()
        });
    let location = match path {
        Some(path) => match origins.get(&path) {
            Some(source) => format!(" at `{path}` from `{source}`: invalid value"),
            None => format!(" at `{path}`: invalid value"),
        },
        None if origins.is_empty() => ": invalid value".to_owned(),
        None => {
            let candidates = origins
                .iter()
                .map(|(path, source)| format!("`{path}` from `{source}`"))
                .collect::<Vec<_>>()
                .join(", ");
            format!(" at one of {candidates}: invalid value")
        }
    };
    ConfigError::Decode { location }
}
