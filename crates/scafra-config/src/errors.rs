use std::{fmt, io};

#[derive(thiserror::Error)]
pub enum ConfigError {
    #[error("could not read configuration file `{path}`: {source}")]
    Read {
        path: String,
        #[source]
        source: io::Error,
    },

    #[error("could not parse {kind} configuration file `{path}`{location}: {message}")]
    Parse {
        path: String,
        kind: &'static str,
        location: String,
        message: &'static str,
    },

    #[error("could not serialize configuration defaults")]
    Serialize,

    #[error("could not decode merged Scafra configuration{location}")]
    Decode { location: String },

    #[error("invalid configuration value for `{path}` from {source_kind}")]
    InvalidValue { path: String, source_kind: String },

    #[error("configuration structure conflict at `{path}` from {source_kind}{location}")]
    StructuralConflict {
        path: String,
        source_kind: String,
        location: String,
    },

    #[error("invalid configuration profile")]
    InvalidProfile,

    #[error("configuration validation failed")]
    Validation(String),
}

impl ConfigError {
    /// Returns validator-approved field and reason details, if available.
    ///
    /// Arbitrary validator errors are omitted by default. Implementations of
    /// [`crate::Config::validation_details`] should only provide details that
    /// do not contain configured values.
    pub fn validation_details(&self) -> Option<&str> {
        match self {
            Self::Validation(details) if !details.is_empty() => Some(details),
            _ => None,
        }
    }
}

impl fmt::Debug for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => f
                .debug_struct("Read")
                .field("path", path)
                .field("source", source)
                .finish(),
            Self::Parse {
                path,
                kind,
                location,
                message,
            } => f
                .debug_struct("Parse")
                .field("path", path)
                .field("kind", kind)
                .field("location", location)
                .field("message", message)
                .finish(),
            Self::Serialize => f.write_str("Serialize"),
            Self::Decode { location } => f
                .debug_struct("Decode")
                .field("location", location)
                .finish(),
            Self::InvalidValue { path, source_kind } => f
                .debug_struct("InvalidValue")
                .field("path", path)
                .field("source_kind", source_kind)
                .finish(),
            Self::StructuralConflict {
                path,
                source_kind,
                location,
            } => f
                .debug_struct("StructuralConflict")
                .field("path", path)
                .field("source_kind", source_kind)
                .field("location", location)
                .finish(),
            Self::InvalidProfile => f.write_str("InvalidProfile"),
            Self::Validation(_) => f.write_str("Validation"),
        }
    }
}
