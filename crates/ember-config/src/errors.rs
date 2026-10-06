use std::io;
#[derive(Debug, thiserror::Error)]
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

    #[error("could not decode merged Ember configuration{location}")]
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
