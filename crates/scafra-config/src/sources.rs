use serde::{de::DeserializeOwned, Serialize};
use std::{
    collections::BTreeMap,
    env, fs, io,
    marker::PhantomData,
    path::{Path, PathBuf},
};

use crate::{
    errors::ConfigError,
    merge::{
        decode_error, merge_properties, merge_yaml, parse_scalar, parse_yaml, set_path,
        validate_profile, SOURCE_EXTENSIONS,
    },
};

const SCAFRA_ENV_CONTROLS: [&str; 3] = ["PROFILE", "DEV_CHILD", "DEV_RELOAD"];
pub fn load_yaml<T: DeserializeOwned>(path: impl AsRef<Path>) -> Result<T, ConfigError> {
    let path = path.as_ref();
    let display_path = path.display().to_string();
    let source = fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: display_path.clone(),
        source,
    })?;
    let value = parse_yaml(&display_path, &source)?;
    serde_yaml::from_value(value).map_err(|_| ConfigError::Decode {
        location: format!(" from `{display_path}`"),
    })
}

/// A typed configuration value can provide its own validation before startup.
pub trait Config: Sized {
    type Error: std::error::Error + Send + Sync + 'static;

    fn validate(&self) -> Result<(), Self::Error>;

    /// Returns safe, actionable validation details when the error contains no
    /// configuration values. The default keeps arbitrary validator errors
    /// redacted.
    fn validation_details(_error: &Self::Error) -> Option<crate::ValidationError> {
        None
    }
}

/// Marks a typed properties struct with the configuration path it owns.
pub trait ConfigProperties: Config {
    const CONFIG_PREFIX: &'static str;
}

/// Lazily accessed, typed application properties for a service or component.
#[derive(Debug, Clone)]
pub struct Properties<T> {
    loader: ConfigLoader,
    marker: PhantomData<fn() -> T>,
}

impl<T> Default for Properties<T> {
    fn default() -> Self {
        Self {
            loader: ConfigLoader::default(),
            marker: PhantomData,
        }
    }
}

impl<T> Properties<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_loader(loader: ConfigLoader) -> Self {
        Self {
            loader,
            marker: PhantomData,
        }
    }

    pub fn load(&self) -> Result<T, ConfigError>
    where
        T: ConfigProperties + DeserializeOwned + Serialize + Default,
        T::Error: std::fmt::Display,
    {
        self.loader.load_properties()
    }
}

/// Loader implementing Scafra's configuration precedence chain:
/// defaults, base YAML/YML/properties files, profile YAML/YML/properties
/// files, sorted environment values, then explicit overrides. Missing files
/// are ignored; every other source failure stops startup.
#[derive(Debug, Clone)]
pub struct ConfigLoader {
    root: PathBuf,
    profile: Option<String>,
    env_prefix: String,
    overrides: BTreeMap<String, String>,
}

impl Default for ConfigLoader {
    fn default() -> Self {
        Self {
            root: PathBuf::from("."),
            profile: None,
            env_prefix: "SCAFRA".to_owned(),
            overrides: BTreeMap::new(),
        }
    }
}

impl ConfigLoader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn root(mut self, root: impl Into<PathBuf>) -> Self {
        self.root = root.into();
        self
    }

    pub fn profile(mut self, profile: impl Into<String>) -> Self {
        self.profile = Some(profile.into());
        self
    }

    pub fn env_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.env_prefix = prefix.into();
        self
    }

    pub fn override_value(mut self, path: impl Into<String>, value: impl Into<String>) -> Self {
        self.overrides.insert(path.into(), value.into());
        self
    }

    /// Returns the active profile from an explicit override or the base
    /// application configuration. Missing profile configuration means
    /// `default`.
    pub fn active_profile(&self) -> Result<String, ConfigError> {
        Ok(self
            .selected_profile()?
            .unwrap_or_else(|| "default".to_owned()))
    }

    pub fn load<T>(&self) -> Result<T, ConfigError>
    where
        T: DeserializeOwned + Serialize + Default,
    {
        let (merged, origins) = self.load_merged::<T>()?;
        serde_yaml::from_value(merged).map_err(|error| decode_error(error, &origins))
    }

    pub fn load_properties<T>(&self) -> Result<T, ConfigError>
    where
        T: ConfigProperties + DeserializeOwned + Serialize + Default,
        T::Error: std::fmt::Display,
    {
        let (merged, origins) = self.load_merged::<serde_yaml::Value>()?;
        let value = if T::CONFIG_PREFIX.is_empty() {
            merged
        } else {
            T::CONFIG_PREFIX
                .split('.')
                .try_fold(&merged, |value, segment| match value {
                    serde_yaml::Value::Mapping(map) => {
                        map.get(serde_yaml::Value::String(segment.to_owned()))
                    }
                    _ => None,
                })
                .cloned()
                .unwrap_or_else(|| {
                    serde_yaml::to_value(T::default()).unwrap_or(serde_yaml::Value::Null)
                })
        };
        let config: T =
            serde_yaml::from_value(value).map_err(|error| decode_error(error, &origins))?;
        config
            .validate()
            .map_err(|error| validation_config_error::<T>(&error))?;
        Ok(config)
    }

    fn load_merged<T>(&self) -> Result<(serde_yaml::Value, BTreeMap<String, String>), ConfigError>
    where
        T: Serialize + Default,
    {
        let profile = self.selected_profile()?;
        validate_profile(profile.as_deref())?;

        let schema = serde_yaml::to_value(T::default()).map_err(|_| ConfigError::Serialize)?;
        let mut merged = schema.clone();
        let mut origins = BTreeMap::new();

        let config_root = self.configuration_root();
        for extension in SOURCE_EXTENSIONS {
            self.merge_file(
                &mut merged,
                &mut origins,
                config_root.join(format!("application.{extension}")),
            )?;
        }
        if let Some(profile) = &profile {
            for extension in SOURCE_EXTENSIONS {
                self.merge_file(
                    &mut merged,
                    &mut origins,
                    config_root.join(format!("application-{profile}.{extension}")),
                )?;
            }
        }

        self.merge_environment(&mut merged, &schema, &mut origins)?;
        for (path, value) in &self.overrides {
            set_path(
                &mut merged,
                path,
                parse_scalar(value),
                "explicit override",
                None,
                &mut origins,
            )?;
        }

        Ok((merged, origins))
    }

    fn selected_profile(&self) -> Result<Option<String>, ConfigError> {
        if let Some(profile) = &self.profile {
            return Ok(Some(profile.clone()));
        }

        let mut base = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
        let mut origins = BTreeMap::new();
        let config_root = self.configuration_root();
        for extension in SOURCE_EXTENSIONS {
            self.merge_file(
                &mut base,
                &mut origins,
                config_root.join(format!("application.{extension}")),
            )?;
        }

        let active = ["scafra", "profiles", "active"]
            .into_iter()
            .try_fold(&base, |value, segment| match value {
                serde_yaml::Value::Mapping(map) => {
                    map.get(serde_yaml::Value::String(segment.to_owned()))
                }
                _ => None,
            })
            .and_then(serde_yaml::Value::as_str)
            .map(str::to_owned);
        Ok(active)
    }

    /// Returns the directory from which application configuration is loaded.
    pub fn configuration_root(&self) -> PathBuf {
        let resources = self.root.join("src").join("resources");
        if resources.is_dir() {
            resources
        } else {
            self.root.clone()
        }
    }

    pub fn load_validated<T>(&self) -> Result<T, ConfigError>
    where
        T: Config + DeserializeOwned + Serialize + Default,
        T::Error: std::fmt::Display,
    {
        let config = self.load::<T>()?;
        config
            .validate()
            .map_err(|error| validation_config_error::<T>(&error))?;
        Ok(config)
    }

    fn merge_file(
        &self,
        target: &mut serde_yaml::Value,
        origins: &mut BTreeMap<String, String>,
        path: PathBuf,
    ) -> Result<(), ConfigError> {
        let display_path = path.display().to_string();
        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(source) => {
                return Err(ConfigError::Read {
                    path: display_path,
                    source,
                })
            }
        };

        if path.extension().and_then(|extension| extension.to_str()) == Some("properties") {
            merge_properties(target, origins, &display_path, &source)
        } else {
            let value = parse_yaml(&display_path, &source)?;
            merge_yaml(target, value, "", &display_path, origins)
        }
    }

    fn merge_environment(
        &self,
        target: &mut serde_yaml::Value,
        schema: &serde_yaml::Value,
        origins: &mut BTreeMap<String, String>,
    ) -> Result<(), ConfigError> {
        let prefix = format!("{}_", self.env_prefix.to_ascii_uppercase());
        let mut variables = env::vars_os()
            .filter_map(|(key, value)| Some((key.into_string().ok()?, value)))
            .filter(|(key, _)| key.starts_with(&prefix))
            .collect::<Vec<_>>();
        variables.sort_by(|left, right| left.0.cmp(&right.0));

        for (key, value) in variables {
            let Some(raw_path) = key.strip_prefix(&prefix) else {
                continue;
            };
            if self.env_prefix.eq_ignore_ascii_case("SCAFRA")
                && SCAFRA_ENV_CONTROLS
                    .iter()
                    .any(|control| raw_path.eq_ignore_ascii_case(control))
            {
                continue;
            }
            let path = resolve_environment_path(schema, raw_path).map_err(|()| {
                ConfigError::InvalidValue {
                    path: raw_path.to_ascii_lowercase(),
                    source_kind: "environment variable name".to_owned(),
                }
            })?;
            if origins
                .get(&path)
                .is_some_and(|source| source == "environment")
            {
                return Err(ConfigError::InvalidValue {
                    path,
                    source_kind: "environment".to_owned(),
                });
            }
            let value = value.into_string().map_err(|_| ConfigError::InvalidValue {
                path: path.clone(),
                source_kind: "environment".to_owned(),
            })?;
            set_path(
                target,
                &path,
                parse_scalar(&value),
                "environment",
                None,
                origins,
            )?;
        }
        Ok(())
    }
}

/// Resolves an environment suffix to a serialized configuration path.
///
/// Double underscores are the explicit path separator. Single-underscore
/// names retain the historical spelling when the serialized schema identifies
/// exactly one path.
fn resolve_environment_path(target: &serde_yaml::Value, raw_path: &str) -> Result<String, ()> {
    if raw_path.is_empty() || !valid_environment_name(raw_path) {
        return Err(());
    }

    if raw_path.contains("__") {
        let parts = raw_path
            .split("__")
            .map(str::to_ascii_lowercase)
            .collect::<Vec<_>>();
        if parts.iter().any(String::is_empty) || !path_exists(target, &parts, "") {
            return Err(());
        }
        return Ok(parts.join("."));
    }

    let tokens = raw_path.split('_').collect::<Vec<_>>();
    if tokens.iter().any(|token| token.is_empty()) {
        return Err(());
    }
    let mut candidates = std::collections::BTreeSet::new();
    legacy_path_candidates(target, &tokens, 0, "", &mut Vec::new(), &mut candidates);
    if candidates.len() != 1 {
        return Err(());
    }
    candidates.into_iter().next().ok_or(())
}

fn valid_environment_name(name: &str) -> bool {
    name.chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
}

fn path_exists(value: &serde_yaml::Value, parts: &[String], parent_path: &str) -> bool {
    let Some((part, remaining)) = parts.split_first() else {
        return !parent_path.is_empty();
    };
    let Some(mapping) = value.as_mapping() else {
        return false;
    };
    let key = serde_yaml::Value::String(part.clone());
    let child_path = append_config_path(parent_path, part);

    if let Some(child) = mapping.get(&key) {
        return remaining.is_empty()
            || path_exists(child, remaining, &child_path)
            || (parent_path == "scheduler.tasks"
                && scheduled_task_schema()
                    .is_some_and(|schema| path_exists(&schema, remaining, &child_path)));
    }
    if is_omitted_secret_field(parent_path, part) {
        return remaining.is_empty();
    }
    if parent_path == "scheduler.tasks" && !remaining.is_empty() {
        return scheduled_task_schema()
            .is_some_and(|schema| path_exists(&schema, remaining, &child_path));
    }
    parent_path != "scheduler.tasks" && mapping.is_empty() && remaining.is_empty()
}

fn legacy_path_candidates(
    value: &serde_yaml::Value,
    tokens: &[&str],
    offset: usize,
    parent_path: &str,
    current_path: &mut Vec<String>,
    candidates: &mut std::collections::BTreeSet<String>,
) {
    if offset == tokens.len() {
        if !current_path.is_empty() {
            candidates.insert(current_path.join("."));
        }
        return;
    }
    let Some(mapping) = value.as_mapping() else {
        return;
    };

    for (key, child) in mapping {
        let Some(key) = key.as_str() else {
            continue;
        };
        let key_tokens = key.split('_').collect::<Vec<_>>();
        let end = offset + key_tokens.len();
        if end > tokens.len()
            || !key_tokens
                .iter()
                .zip(&tokens[offset..end])
                .all(|(left, right)| left.eq_ignore_ascii_case(right))
        {
            continue;
        }
        current_path.push(key.to_owned());
        let child_path = append_config_path(parent_path, key);
        legacy_path_candidates(child, tokens, end, &child_path, current_path, candidates);
        current_path.pop();
    }

    for secret_field in omitted_secret_fields(parent_path) {
        let key_tokens = secret_field.split('_').collect::<Vec<_>>();
        let end = offset + key_tokens.len();
        if end == tokens.len()
            && key_tokens
                .iter()
                .zip(&tokens[offset..end])
                .all(|(left, right)| left.eq_ignore_ascii_case(right))
        {
            current_path.push((*secret_field).to_owned());
            candidates.insert(current_path.join("."));
            current_path.pop();
        }
    }

    if parent_path == "scheduler.tasks" {
        let Some(task_schema) = scheduled_task_schema() else {
            return;
        };
        for end in (offset + 1)..tokens.len() {
            let task_name = tokens[offset..end].join("_").to_ascii_lowercase();
            current_path.push(task_name.clone());
            legacy_path_candidates(
                &task_schema,
                tokens,
                end,
                &append_config_path(parent_path, &task_name),
                current_path,
                candidates,
            );
            current_path.pop();
        }
    } else if parent_path != "scheduler.tasks" && mapping.is_empty() && offset + 1 == tokens.len() {
        let key = tokens[offset].to_ascii_lowercase();
        current_path.push(key);
        candidates.insert(current_path.join("."));
        current_path.pop();
    }
}

fn omitted_secret_fields(parent_path: &str) -> &'static [&'static str] {
    // These serde-skipped keys are intentionally absent from the serialized
    // default, but remain valid deserialization targets for secret injection.
    match parent_path {
        "security.jwt" => &["secret"],
        "security.basic" => &["password"],
        "actuator.security" => &["bearer_token"],
        _ => &[],
    }
}

fn is_omitted_secret_field(parent_path: &str, field: &str) -> bool {
    omitted_secret_fields(parent_path).contains(&field)
}

fn scheduled_task_schema() -> Option<serde_yaml::Value> {
    serde_yaml::to_value(scafra_scheduler::ScheduledTaskConfig::default()).ok()
}

fn append_config_path(parent_path: &str, segment: &str) -> String {
    if parent_path.is_empty() {
        segment.to_owned()
    } else {
        format!("{parent_path}.{segment}")
    }
}

fn validation_config_error<T: Config>(error: &T::Error) -> ConfigError {
    match T::validation_details(error) {
        Some(details) => ConfigError::Validation(format!("{}: {}", details.field, details.message)),
        None => ConfigError::Validation(String::new()),
    }
}
