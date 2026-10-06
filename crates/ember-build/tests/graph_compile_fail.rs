use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

struct TemporaryTarget(PathBuf);

impl TemporaryTarget {
    fn new() -> Self {
        let path = env::temp_dir().join(format!(
            "ember-build-graph-compile-fail-{}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("temporary cargo target should be creatable");
        Self(path)
    }
}

impl Drop for TemporaryTarget {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn invalid_generated_graphs_fail_with_source_aware_diagnostics() {
    let fixtures = [
        (
            "missing",
            "Ember graph is missing dependency `Missing` required by `Service`",
            "src/node.rs:7",
        ),
        (
            "duplicate",
            "Ember graph has duplicate output `Shared`",
            "src/first.rs:4",
        ),
        (
            "cycle",
            "Ember graph contains a dependency cycle: A -> B -> A",
            "src/a.rs:5",
        ),
        (
            "qualified",
            "qualified paths are not supported",
            "src/service.rs:4",
        ),
        (
            "ambiguous",
            "ambiguous public graph type `Shared`",
            "src/first.rs:4",
        ),
    ];
    let target = TemporaryTarget::new();
    let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");

    for (fixture, expected, source) in fixtures {
        let manifest = fixture_root.join(fixture).join("Cargo.toml");
        let output = Command::new("cargo")
            .args(["check", "--offline", "--manifest-path"])
            .arg(&manifest)
            .env("CARGO_TARGET_DIR", &target.0)
            .output()
            .unwrap_or_else(|error| panic!("could not check {fixture} graph fixture: {error}"));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !output.status.success(),
            "{fixture} fixture unexpectedly compiled"
        );
        assert!(stderr.contains(expected), "{fixture} stderr:\n{stderr}");
        assert!(
            stderr.contains(source),
            "{fixture} lost declaration line:\n{stderr}"
        );
        assert!(
            stderr.contains("src/"),
            "{fixture} lost source context:\n{stderr}"
        );
        if fixture == "ambiguous" {
            assert!(
                stderr.contains("src/second.rs:2"),
                "{fixture} did not identify the colliding declaration:\n{stderr}"
            );
            assert!(
                stderr.contains("src/service.rs"),
                "{fixture} did not identify the affected graph declaration:\n{stderr}"
            );
            assert!(
                !stderr.contains("mismatched types"),
                "{fixture} reached a generated constructor mismatch:\n{stderr}"
            );
        }
    }
}
