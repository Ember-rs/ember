use std::{
    fs,
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

use clap::Parser;

use crate::{
    cli::{ApplicationKind, Cli, CommandKind},
    filesystem::{
        escape_toml_basic_string, local_dependency, manifest_path, normalize_package_name,
        package_name, reserve_destination,
    },
    generator::create_project,
    templates::STANDARD_DIRECTORIES,
};

#[test]
fn defaults_new_to_web() {
    let cli = Cli::try_parse_from(["ember", "new", "my-app"]).unwrap();
    assert!(matches!(
        cli.command,
        CommandKind::New {
            kind: ApplicationKind::Web,
            ..
        }
    ));
}

#[test]
fn accepts_microservice_as_service_alias() {
    let cli = Cli::try_parse_from(["ember", "new", "my-app", "--kind", "microservice"]).unwrap();
    assert!(matches!(
        cli.command,
        CommandKind::New {
            kind: ApplicationKind::Service,
            ..
        }
    ));
}

#[test]
fn normalizes_destination_name_for_cargo() {
    assert_eq!(normalize_package_name("billing-api"), "billing_api");
    assert_eq!(normalize_package_name("123 catalog"), "_123_catalog");
    assert_eq!(normalize_package_name("café"), "caf_");
}

#[test]
fn rejects_destination_without_a_final_component() {
    assert!(package_name(Path::new("")).is_err());
}

#[cfg(unix)]
#[test]
fn rejects_destination_with_a_non_utf8_final_component() {
    use std::os::unix::ffi::OsStringExt;

    let path = PathBuf::from(std::ffi::OsString::from_vec(vec![b'g', 0x80]));
    let error = package_name(&path).unwrap_err();

    assert!(error
        .to_string()
        .contains("project path must end in a valid UTF-8 directory name"));
}

#[test]
fn reports_destination_metadata_errors_before_generation() {
    let error =
        create_project(Path::new("invalid\0destination"), ApplicationKind::Web).unwrap_err();
    assert!(error.to_string().contains("could not inspect destination"));
}

#[cfg(unix)]
#[test]
fn refuses_to_follow_a_dangling_destination_symlink() {
    use std::os::unix::fs::symlink;

    let root = test_directory("dangling-symlink");
    fs::create_dir_all(&root).unwrap();
    let destination = root.join("generated");
    let target = root.join("missing-target");
    symlink(&target, &destination).unwrap();

    let error = create_project(&destination, ApplicationKind::Web).unwrap_err();

    assert!(error
        .to_string()
        .contains("refusing to overwrite existing path"));
    assert!(!target.exists());
    assert!(fs::symlink_metadata(&destination)
        .unwrap()
        .file_type()
        .is_symlink());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn serializes_relative_manifest_paths_portably() {
    assert_eq!(
        manifest_path(Path::new("../crates/ember-build")).unwrap(),
        "../crates/ember-build"
    );

    let path_with_quote = Path::new("../crates/ember\"");
    assert_eq!(
        manifest_path(path_with_quote).unwrap(),
        format!("../crates/ember{}", "\\\"")
    );
    assert_eq!(escape_toml_basic_string("\\"), "\\\\");
    assert!(manifest_path(Path::new("/absolute/path")).is_err());
}

#[test]
fn keeps_published_dependency_fallback_when_checkout_crate_is_missing() {
    assert!(
        local_dependency(Path::new("/tmp/generated-app"), "not-an-ember-crate")
            .unwrap()
            .is_none()
    );
}

#[test]
fn generates_each_fixed_starter_shape() {
    let root = test_directory("shapes");
    for kind in [
        ApplicationKind::Web,
        ApplicationKind::Api,
        ApplicationKind::Service,
        ApplicationKind::Monolith,
    ] {
        let project = root.join(kind.as_str());
        create_project(&project, kind).unwrap();

        for directory in STANDARD_DIRECTORIES {
            assert!(project.join(directory).is_dir(), "missing {directory}");
        }
        assert!(project.join("Cargo.toml").is_file());
        assert!(!project.join("build.rs").exists());
        assert!(project.join("src/resources/application.yaml").is_file());
        assert!(project.join("src/main.rs").is_file());
    }

    let web_manifest = fs::read_to_string(root.join("web/Cargo.toml")).unwrap();
    assert!(!web_manifest.contains("serde ="));
    assert!(
        fs::read_to_string(root.join("web/src/main/controllers/hello_controller.rs"))
            .unwrap()
            .contains("/hello/{name}")
    );

    let api_manifest = fs::read_to_string(root.join("api/Cargo.toml")).unwrap();
    assert!(api_manifest.contains("serde = { version = \"1\", features = [\"derive\"] }"));
    assert!(
        fs::read_to_string(root.join("api/src/main/controllers/greeting_controller.rs"))
            .unwrap()
            .contains("/greetings/{name}")
    );
    assert!(
        fs::read_to_string(root.join("service/src/main/controllers/health_controller.rs"))
            .unwrap()
            .contains("/health")
    );
    assert!(root
        .join("monolith/src/main/modules/catalog/catalog_controller.rs")
        .is_file());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn refuses_to_overwrite_existing_destination() {
    let path = test_directory("existing");
    fs::create_dir_all(&path).unwrap();
    let error = create_project(&path, ApplicationKind::Web).unwrap_err();
    assert!(error
        .to_string()
        .contains("refusing to overwrite existing path"));
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn reserves_destination_root_exclusively_after_preflight() {
    let root = test_directory("exclusive");
    let destination = root.join("nested/project");

    reserve_destination(&destination).unwrap();
    assert!(destination.is_dir());

    let error = reserve_destination(&destination).unwrap_err();
    assert!(error
        .to_string()
        .contains("refusing to overwrite existing path"));

    fs::remove_dir_all(root).unwrap();
}

fn test_directory(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("ember-cli-{name}-{}-{nonce}", process::id()))
}
