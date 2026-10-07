use std::{
    collections::BTreeMap,
    error::Error,
    fmt, fs,
    path::{Component, Path, PathBuf},
};

use scafra_foundation::{Phase, PhaseMetadata};

/// The immutable paths available to one build-time extension invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildContext {
    manifest_dir: PathBuf,
    source_root: PathBuf,
    output_dir: PathBuf,
}

impl BuildContext {
    /// Creates a build context from application-owned paths.
    pub fn new(
        manifest_dir: impl Into<PathBuf>,
        source_root: impl Into<PathBuf>,
        output_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            manifest_dir: manifest_dir.into(),
            source_root: source_root.into(),
            output_dir: output_dir.into(),
        }
    }

    /// Returns the Cargo manifest directory.
    pub fn manifest_dir(&self) -> &Path {
        &self.manifest_dir
    }

    /// Returns the resolved application source root.
    pub fn source_root(&self) -> &Path {
        &self.source_root
    }

    /// Returns the Cargo output directory.
    pub fn output_dir(&self) -> &Path {
        &self.output_dir
    }
}

/// A typed, synchronous build-time extension owned by one pipeline run.
pub trait BuildExtension {
    /// Returns the static descriptor used for validation and ordering.
    fn metadata(&self) -> PhaseMetadata;

    /// Applies this extension to the explicit build context and output.
    fn apply(
        &mut self,
        context: &BuildContext,
        output: &mut BuildOutput,
    ) -> Result<(), BuildExtensionError>;
}

/// An author-supplied, controlled extension failure message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildExtensionError {
    message: String,
}

impl BuildExtensionError {
    /// Creates an extension failure from an already controlled message.
    ///
    /// Extension authors are responsible for removing secrets before passing
    /// a message into the build pipeline.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the author-supplied failure message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for BuildExtensionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for BuildExtensionError {}

/// A rejected in-memory artifact path or artifact collision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildOutputError {
    /// The artifact path is not a safe relative file path.
    InvalidPath { path: PathBuf, reason: &'static str },
    /// An artifact with the same relative path was already staged.
    DuplicatePath { path: PathBuf },
}

impl fmt::Display for BuildOutputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath { path, reason } => {
                write!(
                    formatter,
                    "invalid build artifact path `{}`: {reason}",
                    path.display()
                )
            }
            Self::DuplicatePath { path } => {
                write!(
                    formatter,
                    "duplicate build artifact path `{}`",
                    path.display()
                )
            }
        }
    }
}

impl Error for BuildOutputError {}

/// The owned artifact set produced by a build-time pipeline.
#[derive(Debug, Default)]
pub struct BuildOutput {
    artifacts: BTreeMap<PathBuf, Vec<u8>>,
    failure: Option<BuildOutputError>,
}

impl BuildOutput {
    /// Creates an empty artifact set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Stages one generated artifact under a safe relative path.
    ///
    /// The artifact is copied into the output set. The output set retains the
    /// first path failure so the owning pipeline can report it with the
    /// extension metadata that caused it.
    pub fn emit(
        &mut self,
        path: impl Into<PathBuf>,
        contents: impl AsRef<[u8]>,
    ) -> Result<(), BuildOutputError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }

        let path = path.into();
        if let Err(error) = validate_artifact_path(&path) {
            self.failure = Some(error.clone());
            return Err(error);
        }
        if self.artifacts.contains_key(&path) {
            let error = BuildOutputError::DuplicatePath { path };
            self.failure = Some(error.clone());
            return Err(error);
        }
        self.artifacts.insert(path, contents.as_ref().to_vec());
        Ok(())
    }

    /// Returns staged artifacts in deterministic relative-path order.
    pub fn artifacts(&self) -> impl Iterator<Item = (&Path, &[u8])> {
        self.artifacts
            .iter()
            .map(|(path, contents)| (path.as_path(), contents.as_slice()))
    }

    /// Returns one staged artifact by its exact relative path.
    pub fn get(&self, path: impl AsRef<Path>) -> Option<&[u8]> {
        self.artifacts.get(path.as_ref()).map(Vec::as_slice)
    }

    pub(crate) fn failure(&self) -> Option<BuildOutputError> {
        self.failure.clone()
    }

    pub(crate) fn commit(&self, output_dir: &Path) -> Result<(), (PathBuf, std::io::Error)> {
        for (path, contents) in &self.artifacts {
            let destination = output_dir.join(path);
            if let Some(parent) = destination.parent() {
                if let Err(error) = fs::create_dir_all(parent) {
                    return Err((destination, error));
                }
            }
            if let Err(error) = fs::write(&destination, contents) {
                return Err((destination, error));
            }
        }
        Ok(())
    }
}

/// A failure produced while validating, executing, or committing a build
/// extension pipeline.
#[derive(Debug)]
pub enum BuildPipelineError {
    /// An extension declared metadata for a phase owned by another crate.
    InvalidPhase { metadata: PhaseMetadata },
    /// Two extensions declared the same complete deterministic ordering key.
    DuplicateMetadata {
        first: PhaseMetadata,
        duplicate: PhaseMetadata,
    },
    /// An extension returned its controlled failure message.
    ExtensionFailure {
        metadata: PhaseMetadata,
        error: BuildExtensionError,
    },
    /// An artifact path or artifact collision stopped the pipeline.
    OutputFailure {
        metadata: Option<PhaseMetadata>,
        error: BuildOutputError,
    },
    /// Discovery or graph rendering could not produce the standard artifact.
    DiscoveryFailure { source: std::io::Error },
    /// A staged artifact could not be committed to the build output directory.
    CommitFailure {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl fmt::Display for BuildPipelineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPhase { metadata } => write!(
                formatter,
                "build extension `{}` at {} declares phase `{}`; expected `build_time`",
                metadata.name(),
                metadata.source(),
                metadata.phase().name()
            ),
            Self::DuplicateMetadata { first, duplicate } => write!(
                formatter,
                "duplicate build extension metadata key (order {}, name `{}`, source `{}`) conflicts with source `{}`",
                first.order(),
                first.name(),
                first.source(),
                duplicate.source()
            ),
            Self::ExtensionFailure { metadata, error } => write!(
                formatter,
                "build extension `{}` in phase `{}` at {} failed: {error}",
                metadata.name(),
                metadata.phase().name(),
                metadata.source()
            ),
            Self::OutputFailure { metadata, error } => {
                if let Some(metadata) = metadata {
                    write!(
                        formatter,
                        "build extension `{}` at {} produced invalid output: {error}",
                        metadata.name(),
                        metadata.source()
                    )
                } else {
                    write!(formatter, "build output is invalid: {error}")
                }
            }
            Self::DiscoveryFailure { source } => {
                write!(formatter, "Scafra build discovery failed: {source}")
            }
            Self::CommitFailure { path, source } => write!(
                formatter,
                "Scafra build output could not be written to {}: {source}",
                path.display()
            ),
        }
    }
}

impl Error for BuildPipelineError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::DiscoveryFailure { source } | Self::CommitFailure { source, .. } => Some(source),
            Self::ExtensionFailure { error, .. } => Some(error),
            Self::OutputFailure { error, .. } => Some(error),
            Self::InvalidPhase { .. } | Self::DuplicateMetadata { .. } => None,
        }
    }
}

/// Validates, deterministically orders, and executes explicit build
/// extensions.
pub fn run_extensions(
    extensions: Vec<Box<dyn BuildExtension>>,
    context: &BuildContext,
    output: &mut BuildOutput,
) -> Result<(), BuildPipelineError> {
    let mut entries = Vec::with_capacity(extensions.len());
    let mut invalid_phase = None;

    for extension in extensions {
        let metadata = extension.metadata();
        if metadata.phase() != Phase::BuildTime && invalid_phase.is_none() {
            invalid_phase = Some(metadata);
        }
        entries.push((metadata, extension));
    }
    if let Some(metadata) = invalid_phase {
        return Err(BuildPipelineError::InvalidPhase { metadata });
    }

    entries.sort_by(|(left, _), (right, _)| {
        left.order()
            .cmp(&right.order())
            .then_with(|| left.name().cmp(right.name()))
            .then_with(|| left.source().cmp(right.source()))
    });
    for pair in entries.windows(2) {
        let (first, _) = &pair[0];
        let (duplicate, _) = &pair[1];
        if first.order() == duplicate.order()
            && first.name() == duplicate.name()
            && first.source() == duplicate.source()
        {
            return Err(BuildPipelineError::DuplicateMetadata {
                first: *first,
                duplicate: *duplicate,
            });
        }
    }

    if let Some(error) = output.failure() {
        return Err(BuildPipelineError::OutputFailure {
            metadata: None,
            error,
        });
    }

    for (metadata, mut extension) in entries {
        let result = extension.apply(context, output);
        if let Some(error) = output.failure() {
            return Err(BuildPipelineError::OutputFailure {
                metadata: Some(metadata),
                error,
            });
        }
        if let Err(error) = result {
            return Err(BuildPipelineError::ExtensionFailure { metadata, error });
        }
    }
    Ok(())
}

fn validate_artifact_path(path: &Path) -> Result<(), BuildOutputError> {
    if path.as_os_str().is_empty() {
        return Err(invalid_path(path, "the path is empty"));
    }
    if path.is_absolute() {
        return Err(invalid_path(path, "the path must be relative"));
    }

    let text = path
        .to_str()
        .ok_or_else(|| invalid_path(path, "the path must be valid UTF-8"))?;
    if text.contains('\\') || looks_like_windows_prefix(text) {
        return Err(invalid_path(
            path,
            "platform prefixes and backslashes are not allowed",
        ));
    }
    if text.split('/').any(|component| component.is_empty()) {
        return Err(invalid_path(path, "empty path components are not allowed"));
    }
    if text.split('/').any(|component| component == ".") {
        return Err(invalid_path(
            path,
            "current-directory components are not allowed",
        ));
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(invalid_path(
            path,
            "parent traversal and platform roots are not allowed",
        ));
    }
    Ok(())
}

fn looks_like_windows_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn invalid_path(path: &Path, reason: &'static str) -> BuildOutputError {
    BuildOutputError::InvalidPath {
        path: path.to_owned(),
        reason,
    }
}
