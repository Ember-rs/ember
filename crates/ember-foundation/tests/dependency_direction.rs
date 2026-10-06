use serde_json::Value;
use std::{path::PathBuf, process::Command};

#[test]
fn foundation_is_independent_and_phase_owners_depend_on_it_directly() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(&manifest)
        .output()
        .expect("cargo metadata should be available to dependency tests");

    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let metadata: Value =
        serde_json::from_slice(&output.stdout).expect("cargo metadata should be valid JSON");
    let packages = metadata["packages"]
        .as_array()
        .expect("cargo metadata should contain packages");

    let package = |name: &str| {
        packages
            .iter()
            .find(|package| package["name"] == name)
            .unwrap_or_else(|| panic!("cargo metadata is missing package {name}"))
    };
    let dependency_names = |name: &str| {
        package(name)["dependencies"]
            .as_array()
            .expect("cargo metadata package should contain dependencies")
            .iter()
            .map(|dependency| dependency["name"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };

    let foundation_dependencies = dependency_names("ember-foundation");
    assert!(!foundation_dependencies
        .iter()
        .any(|name| name.starts_with("ember")));

    for consumer in ["ember-core", "ember-build", "ember-macros", "ember-cli"] {
        assert!(
            dependency_names(consumer)
                .iter()
                .any(|name| name == "ember-foundation"),
            "{consumer} must declare a direct ember-foundation dependency"
        );
    }
}
