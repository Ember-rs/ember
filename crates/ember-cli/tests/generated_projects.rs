use std::{
    collections::VecDeque,
    env, fmt, fs,
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream},
    panic::{self, AssertUnwindSafe},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Output, Stdio},
    sync::mpsc::{self, RecvTimeoutError, TryRecvError},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const STANDARD_DIRECTORIES: &[&str] = &[
    "src",
    "src/main",
    "src/main/controllers",
    "src/main/services",
    "src/main/repositories",
    "src/main/beans",
    "src/main/config",
    "src/main/models",
    "src/main/errors",
    "src/resources",
    "tests",
];

#[derive(Clone, Copy)]
enum Kind {
    Web,
    Api,
    Service,
    Monolith,
}

impl Kind {
    fn cli_value(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::Api => "api",
            Self::Service => "service",
            Self::Monolith => "monolith",
        }
    }

    fn specific_files(self) -> &'static [&'static str] {
        match self {
            Self::Web => &[
                "src/main/beans/greeting_prefix.rs",
                "src/main/services/hello_service.rs",
                "src/main/controllers/hello_controller.rs",
            ],
            Self::Api => &[
                "src/main/models/greeting.rs",
                "src/main/services/greeting_service.rs",
                "src/main/controllers/greeting_controller.rs",
            ],
            Self::Service => &["src/main/controllers/health_controller.rs"],
            Self::Monolith => &[
                "src/main/modules/catalog/catalog_model.rs",
                "src/main/modules/catalog/catalog_service.rs",
                "src/main/modules/catalog/catalog_controller.rs",
            ],
        }
    }

    fn route(self) -> &'static str {
        match self {
            Self::Web => "/hello/Alice",
            Self::Api => "/api/greetings/Alice",
            Self::Service => "/health",
            Self::Monolith => "/catalog/items",
        }
    }

    fn expected_body(self) -> &'static str {
        match self {
            Self::Web => "Hello, Alice!",
            Self::Api => r#"{"message":"Hello, Alice!"}"#,
            Self::Service => "ok",
            Self::Monolith => r#"[{"id":1,"name":"Notebook"},{"id":2,"name":"Pen"}]"#,
        }
    }
}

#[test]
fn cli_generates_compiling_projects_for_every_shape() {
    let workspace = TestWorkspace::new("all-shapes");
    let target_dir = workspace.root.join("shared-target");

    for kind in [Kind::Web, Kind::Api, Kind::Service, Kind::Monolith] {
        let project = workspace.root.join(kind.cli_value());
        run_cli(&["new", project.to_str().unwrap(), "--kind", kind.cli_value()]);
        assert_project_layout(&project, kind);
        cargo_check(&project, &target_dir);
    }
}

#[test]
fn cli_preserves_default_and_alias_and_normalizes_package_names() {
    let workspace = TestWorkspace::new("compatibility");
    let target_dir = workspace.root.join("shared-target");

    let default_project = workspace.root.join("default-project");
    run_cli(&["new", default_project.to_str().unwrap()]);
    assert_project_layout(&default_project, Kind::Web);
    cargo_check(&default_project, &target_dir);

    let alias_project = workspace.root.join("alias-project");
    run_cli(&[
        "new",
        alias_project.to_str().unwrap(),
        "--kind",
        "microservice",
    ]);
    assert_project_layout(&alias_project, Kind::Service);
    cargo_check(&alias_project, &target_dir);
    assert!(fs::read_to_string(alias_project.join("README.md"))
        .unwrap()
        .contains("GET /health"));

    let normalized_project = workspace.root.join("123 catalog");
    run_cli(&["new", normalized_project.to_str().unwrap(), "--kind", "api"]);
    let manifest = fs::read_to_string(normalized_project.join("Cargo.toml")).unwrap();
    assert!(manifest.contains("name = \"_123_catalog\""));
    assert!(manifest.contains("ember = { package = \"ember-framework\", path ="));
    assert!(!manifest.contains("ember-build"));
}

#[test]
fn documented_in_repository_generation_isolated_from_parent_workspace() {
    let repository = fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .expect("CLI integration test should run inside the repository");
    let project_name = format!(
        "ember-generated-workspace-test-{}-{}",
        process_id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let project = repository.join(&project_name);
    let _cleanup = RemoveOnDrop(project.clone());
    let target_workspace = TestWorkspace::new("workspace-isolation");

    run_cli_in(&repository, &["new", project_name.as_str()]);
    let manifest = fs::read_to_string(project.join("Cargo.toml")).unwrap();
    assert!(manifest.contains("\n[workspace]\n"));
    cargo_check(&project, &target_workspace.root.join("target"));
}

#[test]
fn generated_project_loads_application_defaults() {
    let workspace = TestWorkspace::new("configuration-defaults");
    let target_dir = workspace.root.join("shared-target");
    let project = workspace.root.join("web");

    run_cli(&["new", project.to_str().unwrap()]);
    fs::write(
        project.join("tests/configuration.rs"),
        r#"use ember::{ConfigLoader, EmberConfig};

#[test]
fn application_defaults_are_loaded() {
    let config = ConfigLoader::new().load::<EmberConfig>().unwrap();
    assert_eq!(config.server.host, "127.0.0.1");
    assert_eq!(config.server.port, 8080);
}
"#,
    )
    .unwrap();

    let output = Command::new("cargo")
        .args(["test", "--offline", "--manifest-path"])
        .arg(project.join("Cargo.toml"))
        .args(["--test", "configuration"])
        .env("CARGO_TARGET_DIR", &target_dir)
        .env_remove("EMBER_PROFILE")
        .env_remove("EMBER_SERVER_HOST")
        .env_remove("EMBER_SERVER_PORT")
        .env_remove("EMBER_LOGGING_LEVEL")
        .output()
        .unwrap_or_else(|error| panic!("could not start generated configuration test: {error}"));
    assert!(
        output.status.success(),
        "generated default configuration test failed:\n{}\n{}",
        text(&output.stdout),
        text(&output.stderr)
    );
}

#[test]
fn generated_project_honors_server_port_environment_override() {
    let workspace = TestWorkspace::new("configuration");
    let target_dir = workspace.root.join("shared-target");
    let project = workspace.root.join("web");

    run_cli(&["new", project.to_str().unwrap()]);
    fs::write(
        project.join("tests/configuration.rs"),
        r#"use ember::{ConfigLoader, EmberConfig};

#[test]
fn server_port_environment_override_is_loaded() {
    let config = ConfigLoader::new().load::<EmberConfig>().unwrap();
    assert_eq!(config.server.host, "127.0.0.1");
    assert_eq!(config.server.port, 9001);
}
"#,
    )
    .unwrap();

    let output = Command::new("cargo")
        .args(["test", "--offline", "--manifest-path"])
        .arg(project.join("Cargo.toml"))
        .args(["--test", "configuration"])
        .env("CARGO_TARGET_DIR", &target_dir)
        .env_remove("EMBER_PROFILE")
        .env_remove("EMBER_SERVER_HOST")
        .env("EMBER_SERVER_PORT", "9001")
        .env_remove("EMBER_LOGGING_LEVEL")
        .output()
        .unwrap_or_else(|error| panic!("could not start generated configuration test: {error}"));
    assert!(
        output.status.success(),
        "generated configuration test failed:\n{}\n{}",
        text(&output.stdout),
        text(&output.stderr)
    );
}

#[test]
fn generated_project_accepts_yml_and_properties_configuration_sources() {
    let workspace = TestWorkspace::new("configuration-source-forms");
    let target_dir = workspace.root.join("shared-target");
    let project = workspace.root.join("web");

    run_cli(&["new", project.to_str().unwrap()]);
    fs::remove_file(project.join("src/resources/application.yaml")).unwrap();
    fs::write(
        project.join("src/resources/application.yml"),
        "server:\n  host: 127.0.0.1\n  port: 8181\n",
    )
    .unwrap();
    fs::write(
        project.join("tests/configuration.rs"),
        r#"use ember::{ConfigLoader, EmberConfig};

#[test]
fn yml_source_is_loaded() {
    let config = ConfigLoader::new().load::<EmberConfig>().unwrap();
    assert_eq!(config.server.port, 8181);
}
"#,
    )
    .unwrap();

    let yml_output = Command::new("cargo")
        .args(["test", "--offline", "--manifest-path"])
        .arg(project.join("Cargo.toml"))
        .args(["--test", "configuration"])
        .env("CARGO_TARGET_DIR", &target_dir)
        .env_remove("EMBER_PROFILE")
        .env_remove("EMBER_SERVER_HOST")
        .env_remove("EMBER_SERVER_PORT")
        .env_remove("EMBER_LOGGING_LEVEL")
        .output()
        .unwrap_or_else(|error| panic!("could not start generated YML test: {error}"));

    fs::remove_file(project.join("src/resources/application.yml")).unwrap();
    fs::write(
        project.join("src/resources/application.properties"),
        "server.host=127.0.0.1\nserver.port=8182\n",
    )
    .unwrap();
    fs::write(
        project.join("tests/configuration.rs"),
        r#"use ember::{ConfigLoader, EmberConfig};

#[test]
fn properties_source_is_loaded() {
    let config = ConfigLoader::new().load::<EmberConfig>().unwrap();
    assert_eq!(config.server.port, 8182);
}
"#,
    )
    .unwrap();

    let properties_output = Command::new("cargo")
        .args(["test", "--offline", "--manifest-path"])
        .arg(project.join("Cargo.toml"))
        .args(["--test", "configuration"])
        .env("CARGO_TARGET_DIR", &target_dir)
        .env_remove("EMBER_PROFILE")
        .env_remove("EMBER_SERVER_HOST")
        .env_remove("EMBER_SERVER_PORT")
        .env_remove("EMBER_LOGGING_LEVEL")
        .output()
        .unwrap_or_else(|error| panic!("could not start generated properties test: {error}"));

    let mut failures = String::new();
    if !yml_output.status.success() {
        failures.push_str(&format!(
            "generated YML configuration test failed:\n{}\n{}\n",
            text(&yml_output.stdout),
            text(&yml_output.stderr)
        ));
    }
    if !properties_output.status.success() {
        failures.push_str(&format!(
            "generated properties configuration test failed:\n{}\n{}\n",
            text(&properties_output.stdout),
            text(&properties_output.stderr)
        ));
    }
    assert!(failures.is_empty(), "{failures}");
}

#[test]
fn cli_rejects_invalid_kind_without_creating_a_destination() {
    let workspace = TestWorkspace::new("invalid-kind");
    let project = workspace.root.join("not-created");
    let output = run_cli_failure(&["new", project.to_str().unwrap(), "--kind", "unknown"]);

    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("invalid value 'unknown'"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("possible values:"), "stderr: {stderr}");
    assert!(!project.exists());
}

#[test]
fn cli_refuses_existing_destination_without_overwriting_it() {
    let workspace = TestWorkspace::new("existing");
    let project = workspace.root.join("already-there");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("keep.txt"), "keep me").unwrap();

    let output = run_cli_failure(&["new", project.to_str().unwrap(), "--kind", "web"]);

    let stderr = text(&output.stderr);
    assert!(stderr.contains("refusing to overwrite existing path"));
    assert_eq!(
        fs::read_to_string(project.join("keep.txt")).unwrap(),
        "keep me"
    );
    assert!(!project.join("Cargo.toml").exists());
}

#[test]
fn cli_reports_destination_write_failure_without_overwriting_parent_file() {
    let workspace = TestWorkspace::new("write-failure");
    let parent = workspace.root.join("parent-file");
    fs::write(&parent, "keep me").unwrap();
    let project = parent.join("generated");

    let output = run_cli_failure(&["new", project.to_str().unwrap(), "--kind", "web"]);

    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("could not inspect destination"),
        "stderr: {stderr}"
    );
    assert_eq!(fs::read_to_string(parent).unwrap(), "keep me");
    assert!(!project.exists());
}

#[cfg(unix)]
#[test]
fn child_guard_reaps_a_live_process_when_scope_unwinds() {
    let child = Command::new("sleep")
        .arg("30")
        .spawn()
        .expect("sleep should be available for the cleanup test");
    let pid = child.id();

    {
        let _guard = ChildGuard::new(child, Kind::Web);
    }

    let status = Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("kill should be available for the cleanup test");
    assert!(!status.success(), "cleanup left process {pid} running");
}

#[cfg(unix)]
#[test]
fn child_guard_marks_cleanup_complete_only_after_reaping() {
    let child = Command::new("sleep")
        .arg("30")
        .spawn()
        .expect("sleep should be available for the cleanup state test");
    let mut guard = ChildGuard::new(child, Kind::Web);

    assert!(!guard.cleanup_complete);
    guard
        .force_cleanup("cleanup state test")
        .expect("live child should be killed and reaped");
    assert!(guard.cleanup_complete);
}

#[cfg(unix)]
#[test]
fn startup_diagnostic_read_is_bounded_before_eof() {
    let child = Command::new("sleep")
        .arg("30")
        .stdout(Stdio::piped())
        .spawn()
        .expect("sleep should be available for the diagnostic test");
    let mut child = ChildGuard::new(child, Kind::Web);

    let started = Instant::now();
    let diagnostic = child.startup_diagnostic();

    assert!(
        started.elapsed() < STARTUP_DIAGNOSTIC_TIMEOUT + Duration::from_secs(1),
        "diagnostic read exceeded its deadline: {:?}",
        started.elapsed()
    );
    assert!(
        diagnostic.contains("timed out"),
        "diagnostic should report its bounded read timeout: {diagnostic}"
    );
    assert_eq!(
        child.diagnostic_readers.len(),
        1,
        "timed-out reader must remain owned by the child guard"
    );

    child
        .force_cleanup("diagnostic test")
        .expect("diagnostic test child should be reapable after cleanup");
    assert!(child.diagnostic_readers.is_empty());
}

#[cfg(unix)]
#[test]
fn child_guard_stays_incomplete_when_a_diagnostic_reader_cannot_join() {
    let child = Command::new("sleep")
        .arg("30")
        .spawn()
        .expect("sleep should be available for the diagnostic join test");
    let mut guard = ChildGuard::new(child, Kind::Web);
    let (_sender, receiver) = mpsc::sync_channel(1);
    let (cancel_sender, _cancel_receiver) = mpsc::sync_channel(1);
    guard.diagnostic_readers.push(DiagnosticReader {
        receiver,
        join_handle: Some(thread::spawn(|| panic!("diagnostic reader failed"))),
        cancel_sender,
        join_error: None,
    });

    let error = guard
        .force_cleanup("diagnostic join failure")
        .expect_err("a failed diagnostic reader join must fail cleanup");

    assert!(matches!(
        error.failure,
        CleanupFailure::Diagnostic(detail) if detail == "diagnostic reader thread panicked"
    ));
    assert_eq!(error.state, ExitState::Reaped);
    assert!(!guard.cleanup_complete);
}

#[cfg(unix)]
#[test]
fn cleanup_error_preserves_process_and_diagnostic_failures() {
    let child = Command::new("sleep")
        .arg("30")
        .spawn()
        .expect("sleep should be available for the combined cleanup test");
    let mut guard = ChildGuard::new(child, Kind::Service);
    let process_error = CleanupError {
        kind: "service",
        pid: guard.child.id(),
        phase: "case completion",
        state: ExitState::Unknown,
        failure: CleanupFailure::Inspection("wait unavailable".to_owned()),
    };

    let error = guard
        .finish_cleanup(
            "case completion",
            Err(process_error),
            Err("diagnostic reader thread panicked".to_owned()),
        )
        .expect_err("combined cleanup failures must remain visible");

    assert!(!guard.cleanup_complete);
    assert!(guard.cleanup_failure_observed);
    assert!(matches!(
        error.failure,
        CleanupFailure::Combined { ref process, ref diagnostic }
            if matches!(process.as_ref(), CleanupFailure::Inspection(detail) if detail == "wait unavailable")
                && diagnostic == "diagnostic reader thread panicked"
    ));
    assert_cleanup_diagnostic(
        &error.to_string(),
        "service",
        guard.child.id(),
        "case completion",
        "unknown",
    );
    assert!(error.to_string().contains("wait unavailable"));
    assert!(error
        .to_string()
        .contains("diagnostic reader thread panicked"));
}

#[cfg(unix)]
#[test]
fn child_guard_bounds_reader_join_and_allows_cleanup_retry() {
    let child = Command::new("sleep")
        .arg("30")
        .spawn()
        .expect("sleep should be available for the bounded diagnostic join test");
    let mut guard = ChildGuard::new(child, Kind::Web);
    let (_sender, receiver) = mpsc::sync_channel(1);
    let (cancel_sender, _cancel_receiver) = mpsc::sync_channel(1);
    let (release_sender, release_receiver) = mpsc::sync_channel(0);
    guard.diagnostic_readers.push(DiagnosticReader {
        receiver,
        join_handle: Some(thread::spawn(move || {
            release_receiver
                .recv()
                .expect("the test should release the blocked reader");
        })),
        cancel_sender,
        join_error: None,
    });

    let started = Instant::now();
    let error = guard
        .force_cleanup("bounded diagnostic join")
        .expect_err("a reader that ignores cancellation must keep cleanup incomplete");
    assert!(
        started.elapsed()
            < FORCEFUL_SHUTDOWN_TIMEOUT + DIAGNOSTIC_JOIN_TIMEOUT + Duration::from_secs(1),
        "reader join exceeded its bounded cleanup budget: {:?}",
        started.elapsed()
    );
    assert!(matches!(
        error.failure,
        CleanupFailure::Diagnostic(detail) if detail.contains("did not terminate")
    ));
    assert!(!guard.cleanup_complete);

    release_sender
        .send(())
        .expect("the blocked reader should still be joinable");
    guard
        .force_cleanup("bounded diagnostic join retry")
        .expect("cleanup should become complete after the reader terminates");
    assert!(guard.cleanup_complete);
}

#[test]
fn generated_project_startup_failure_is_bounded_and_reaped() {
    let workspace = TestWorkspace::new("startup-failure");
    let target_dir = workspace.root.join("shared-target");
    let project = workspace.root.join("web");

    run_cli(&["new", project.to_str().unwrap()]);
    cargo_build(&project, &target_dir);

    let executable = target_dir.join("debug").join("web");
    let child = Command::new(&executable)
        .current_dir(&project)
        .env("EMBER_SERVER_PORT", "not-a-port")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("could not start {}: {error}", executable.display()));
    let mut child = ChildGuard::new(child, Kind::Web);

    let started = Instant::now();
    let status = wait_for_exit(
        &mut child.child,
        STARTUP_DIAGNOSTIC_TIMEOUT + Duration::from_secs(1),
        Kind::Web,
    )
    .expect("invalid configuration should terminate the generated process within the bounded startup budget");
    let diagnostic = child.startup_diagnostic();

    assert!(
        !status.success(),
        "invalid configuration unexpectedly started"
    );
    assert!(
        started.elapsed() < STARTUP_DIAGNOSTIC_TIMEOUT + Duration::from_secs(2),
        "startup failure exceeded its bounded diagnostic budget: {:?}",
        started.elapsed()
    );
    assert!(
        !diagnostic.contains("timed out"),
        "startup failure diagnostic was not bounded successfully: {diagnostic}"
    );
    assert!(
        diagnostic == "stdout: <no output>; stderr: <no output>"
            || diagnostic.contains("Ember application failed")
            || diagnostic.contains("invalid configured server address"),
        "startup failure diagnostic should remain bounded and stable: {diagnostic}"
    );

    child
        .force_cleanup("startup failure completion")
        .expect("an already-reaped startup failure must not require forced cleanup");
}

#[test]
fn cleanup_reports_initial_inspection_failure_even_after_reaping() {
    let mut process = FakeCleanupProcess::new(
        4_242,
        [
            Err(io::Error::other("inspection unavailable")),
            Ok(Some(())),
        ],
        None,
    );

    let error = cleanup_process(&mut process, "web", "assertion", Duration::ZERO)
        .expect_err("an inspection error must not be hidden by later reaping");

    assert_eq!(error.kind, "web");
    assert_eq!(error.pid, 4_242);
    assert_eq!(error.phase, "assertion");
    assert_eq!(error.state, ExitState::Reaped);
    assert!(matches!(
        &error.failure,
        CleanupFailure::Inspection(detail) if detail.contains("inspection unavailable")
    ));
    assert_cleanup_diagnostic(&error.to_string(), "web", 4_242, "assertion", "reaped");
}

#[test]
fn cleanup_accepts_an_already_reaped_child_without_forcing_kill() {
    let mut process = FakeCleanupProcess::new(4_241, [Ok(Some(()))], Some("must not kill"));

    cleanup_process(&mut process, "web", "already exited", Duration::ZERO)
        .expect("an already-reaped child needs no forced cleanup");

    assert_eq!(process.kill_calls, 0);
}

#[test]
fn cleanup_reaps_a_child_after_successful_force_kill() {
    let mut process = FakeCleanupProcess::new(4_240, [Ok(None), Ok(Some(()))], None);

    cleanup_process(&mut process, "api", "assertion", Duration::ZERO)
        .expect("a successfully killed child should be reaped");

    assert_eq!(process.kill_calls, 1);
}

#[test]
fn cleanup_reports_force_kill_failure_after_reaping() {
    let mut process =
        FakeCleanupProcess::new(4_243, [Ok(None), Ok(Some(()))], Some("permission denied"));

    let error = cleanup_process(&mut process, "service", "startup", Duration::ZERO)
        .expect_err("a force-kill error must fail cleanup even when the child exits");

    assert_eq!(error.state, ExitState::Reaped);
    assert!(matches!(
        &error.failure,
        CleanupFailure::Kill(detail) if detail.contains("permission denied")
    ));
    assert_cleanup_diagnostic(&error.to_string(), "service", 4_243, "startup", "reaped");
}

#[test]
fn cleanup_reports_force_kill_failure_for_unreaped_child() {
    let mut process =
        FakeCleanupProcess::new(4_246, [Ok(None), Ok(None)], Some("permission denied"));

    let error = cleanup_process(&mut process, "web", "startup", Duration::ZERO)
        .expect_err("a failed force-kill must fail when the child remains running");

    assert_eq!(error.state, ExitState::Running);
    assert!(matches!(
        &error.failure,
        CleanupFailure::Kill(detail) if detail.contains("permission denied")
    ));
    assert_cleanup_diagnostic(&error.to_string(), "web", 4_246, "startup", "running");
}

#[test]
fn cleanup_reports_cleanup_timeout_for_unreaped_child() {
    let mut process = FakeCleanupProcess::new(4_244, [Ok(None), Ok(None)], None);

    let error = cleanup_process(&mut process, "api", "request assertion", Duration::ZERO)
        .expect_err("a child that remains running must fail bounded cleanup");

    assert_eq!(error.state, ExitState::Running);
    assert!(matches!(
        error.failure,
        CleanupFailure::Timeout(Duration::ZERO)
    ));
    assert_cleanup_diagnostic(
        &error.to_string(),
        "api",
        4_244,
        "request assertion",
        "running",
    );
}

#[test]
fn cleanup_reports_reaping_inspection_failure_with_unknown_state() {
    let mut process = FakeCleanupProcess::new(
        4_245,
        [Ok(None), Err(io::Error::other("wait unavailable"))],
        None,
    );

    let error = cleanup_process(&mut process, "monolith", "panic", Duration::ZERO)
        .expect_err("an unreapable inspection must fail cleanup");

    assert_eq!(error.state, ExitState::Unknown);
    assert!(matches!(
        &error.failure,
        CleanupFailure::Inspection(detail) if detail.contains("wait unavailable")
    ));
    assert_cleanup_diagnostic(&error.to_string(), "monolith", 4_245, "panic", "unknown");
}

fn assert_cleanup_diagnostic(diagnostic: &str, kind: &str, pid: u32, phase: &str, state: &str) {
    assert!(diagnostic.contains(kind), "diagnostic: {diagnostic}");
    assert!(
        diagnostic.contains(&format!("pid {pid}")),
        "diagnostic: {diagnostic}"
    );
    assert!(diagnostic.contains(phase), "diagnostic: {diagnostic}");
    assert!(
        diagnostic.contains(&format!("exit state: {state}")),
        "diagnostic: {diagnostic}"
    );
}

struct FakeCleanupProcess {
    pid: u32,
    wait_results: VecDeque<io::Result<Option<()>>>,
    kill_error: Option<io::Error>,
    kill_calls: usize,
}

impl FakeCleanupProcess {
    fn new(
        pid: u32,
        wait_results: impl IntoIterator<Item = io::Result<Option<()>>>,
        kill_error: Option<&str>,
    ) -> Self {
        Self {
            pid,
            wait_results: wait_results.into_iter().collect(),
            kill_error: kill_error.map(io::Error::other),
            kill_calls: 0,
        }
    }
}

impl CleanupProcess for FakeCleanupProcess {
    fn cleanup_pid(&self) -> u32 {
        self.pid
    }

    fn cleanup_try_wait(&mut self) -> io::Result<Option<()>> {
        self.wait_results.pop_front().unwrap_or(Ok(None))
    }

    fn cleanup_kill(&mut self) -> io::Result<()> {
        self.kill_calls += 1;
        match self.kill_error.take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

#[cfg(unix)]
#[test]
#[ignore = "requires loopback binding; run in an environment that permits local listeners"]
fn generated_projects_serve_routes_and_shutdown_gracefully() {
    let workspace = TestWorkspace::new("runtime-smoke");
    let target_dir = workspace.root.join("shared-target");
    let cases = [
        (Kind::Web, 18_081),
        (Kind::Api, 18_082),
        (Kind::Service, 18_083),
        (Kind::Monolith, 18_084),
    ];

    for (kind, port) in cases {
        let project = workspace.root.join(kind.cli_value());
        run_cli(&["new", project.to_str().unwrap(), "--kind", kind.cli_value()]);
        cargo_build(&project, &target_dir);

        let executable = target_dir.join("debug").join(kind.cli_value());
        let child = Command::new(&executable)
            .current_dir(&project)
            .env("EMBER_SERVER_PORT", port.to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|error| panic!("could not start {}: {error}", executable.display()));
        run_runtime_case(child, kind, port);
    }
}

fn run_runtime_case(child: Child, kind: Kind, port: u16) {
    let mut child = ChildGuard::new(child, kind);
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        let response = match wait_for_response(child.as_mut(), port, kind.route()) {
            Some(response) => response,
            None => {
                let outcome = terminate(child.as_mut(), kind);
                let diagnostic = child.startup_diagnostic();
                panic!(
                    "{} did not serve {}; shutdown cleanup graceful={}, forced={}, exit status was {:?}; startup diagnostic: {}",
                    kind.cli_value(),
                    kind.route(),
                    outcome.graceful,
                    outcome.forced,
                    outcome.status,
                    diagnostic
                );
            }
        };
        terminate_with_success(child.as_mut(), kind);
        assert_response(kind, &response);
    }));
    let cleanup = child.force_cleanup("case completion");
    drop(child);

    match (result, cleanup) {
        (Ok(()), Ok(())) => {}
        (Ok(()), Err(error)) => panic!("{error}"),
        (Err(panic), Ok(())) => panic::resume_unwind(panic),
        (Err(panic), Err(cleanup)) => panic!(
            "runtime smoke failed: {}; process cleanup also failed: {cleanup}",
            panic_message(&panic)
        ),
    }
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = panic.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = panic.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}

fn assert_response(kind: Kind, response: &str) {
    let (headers, body) = response
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("response has no HTTP header/body separator: {response}"));
    assert!(headers.starts_with("HTTP/1.1 200"), "response: {response}");

    let content_type = headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-type")
            .then_some(value.trim())
    });

    match kind {
        Kind::Web | Kind::Service => {
            assert_eq!(
                content_type,
                Some("text/plain; charset=utf-8"),
                "response: {response}"
            );
            assert_eq!(body, kind.expected_body());
        }
        Kind::Api => {
            assert_eq!(
                content_type,
                Some("application/json"),
                "response: {response}"
            );
            let payload: serde_json::Value =
                serde_json::from_str(body).expect("API response should be valid JSON");
            assert_eq!(payload, serde_json::json!({"message": "Hello, Alice!"}));
        }
        Kind::Monolith => {
            assert_eq!(
                content_type,
                Some("application/json"),
                "response: {response}"
            );
            let payload: serde_json::Value =
                serde_json::from_str(body).expect("monolith response should be valid JSON");
            assert_eq!(
                payload,
                serde_json::json!([
                    {"id": 1, "name": "Notebook"},
                    {"id": 2, "name": "Pen"}
                ])
            );
        }
    }
}

#[test]
fn response_contract_assertions_cover_every_shape() {
    for kind in [Kind::Web, Kind::Api, Kind::Service, Kind::Monolith] {
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: {}\r\n\r\n{}",
            match kind {
                Kind::Web | Kind::Service => "text/plain; charset=utf-8",
                Kind::Api | Kind::Monolith => "application/json",
            },
            kind.expected_body()
        );

        assert_response(kind, &response);
    }
}

fn assert_project_layout(project: &Path, kind: Kind) {
    for directory in STANDARD_DIRECTORIES {
        assert!(project.join(directory).is_dir(), "missing {directory}");
    }

    for file in [
        "Cargo.toml",
        "src/resources/application.yaml",
        "README.md",
        "src/main.rs",
    ] {
        assert!(project.join(file).is_file(), "missing {file}");
    }
    for file in kind.specific_files() {
        assert!(project.join(file).is_file(), "missing {file}");
    }

    let readme = fs::read_to_string(project.join("README.md")).unwrap();
    assert!(readme.contains(&format!("GET {}", kind.route())));
    assert!(readme.contains("EMBER_SERVER_PORT=9000 cargo run"));

    let manifest = fs::read_to_string(project.join("Cargo.toml")).unwrap();
    let has_serde = manifest.contains("serde = { version = \"1\", features = [\"derive\"] }");
    assert_eq!(has_serde, matches!(kind, Kind::Api | Kind::Monolith));

    let application = fs::read_to_string(project.join("src/resources/application.yaml")).unwrap();
    assert!(application.contains("host: 127.0.0.1"));
    assert!(application.contains("port: 8080"));
    assert!(application.contains("level: info"));
}

fn cargo_check(project: &Path, target_dir: &Path) {
    let output = Command::new("cargo")
        .args(["check", "--offline", "--manifest-path"])
        .arg(project.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", target_dir)
        .output()
        .unwrap_or_else(|error| panic!("could not start cargo check: {error}"));
    assert!(
        output.status.success(),
        "cargo check failed for {}:\n{}\n{}",
        project.display(),
        text(&output.stdout),
        text(&output.stderr)
    );
}

fn cargo_build(project: &Path, target_dir: &Path) {
    let output = Command::new("cargo")
        .args(["build", "--offline", "--manifest-path"])
        .arg(project.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(target_dir)
        .output()
        .unwrap_or_else(|error| panic!("could not start cargo build: {error}"));
    assert!(
        output.status.success(),
        "cargo build failed for {}:\n{}\n{}",
        project.display(),
        text(&output.stdout),
        text(&output.stderr)
    );
}

fn wait_for_response(child: &mut Child, port: u16, route: &str) -> Option<String> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    for _ in 0..30 {
        if let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(250)) {
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let request = format!(
                "GET {route} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
            );
            stream.write_all(request.as_bytes()).unwrap();
            let mut response = Vec::new();
            let _ = stream.read_to_end(&mut response);
            return Some(text(&response));
        }
        if child.try_wait().unwrap().is_some() {
            return None;
        }
        thread::sleep(Duration::from_millis(250));
    }
    None
}

const GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const FORCEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);
const STARTUP_DIAGNOSTIC_TIMEOUT: Duration = Duration::from_millis(250);
const DIAGNOSTIC_JOIN_TIMEOUT: Duration = Duration::from_millis(250);
const MAX_STARTUP_DIAGNOSTIC_BYTES: u64 = 16 * 1024;

struct ChildGuard {
    child: Child,
    kind: Kind,
    diagnostic_readers: Vec<DiagnosticReader>,
    cleanup_complete: bool,
    cleanup_failure_observed: bool,
}

impl ChildGuard {
    fn new(child: Child, kind: Kind) -> Self {
        Self {
            child,
            kind,
            diagnostic_readers: Vec::new(),
            cleanup_complete: false,
            cleanup_failure_observed: false,
        }
    }

    fn as_mut(&mut self) -> &mut Child {
        &mut self.child
    }

    fn startup_diagnostic(&mut self) -> String {
        let stdout = match self.child.stdout.take().map(spawn_bounded_diagnostic) {
            Some(mut reader) => {
                let (diagnostic, reader_pending) = collect_bounded_diagnostic(&mut reader);
                if reader_pending {
                    self.diagnostic_readers.push(reader);
                }
                diagnostic
            }
            None => "<stdout unavailable>".to_owned(),
        };
        let stderr = match self.child.stderr.take().map(spawn_bounded_diagnostic) {
            Some(mut reader) => {
                let (diagnostic, reader_pending) = collect_bounded_diagnostic(&mut reader);
                if reader_pending {
                    self.diagnostic_readers.push(reader);
                }
                diagnostic
            }
            None => "<stderr unavailable>".to_owned(),
        };
        format!("stdout: {stdout}; stderr: {stderr}")
    }

    fn force_cleanup(&mut self, phase: &'static str) -> Result<(), CleanupError> {
        let cleanup = cleanup_process(
            &mut self.child,
            self.kind.cli_value(),
            phase,
            FORCEFUL_SHUTDOWN_TIMEOUT,
        );
        let diagnostics = self.join_diagnostic_readers();
        self.finish_cleanup(phase, cleanup, diagnostics)
    }

    fn finish_cleanup(
        &mut self,
        phase: &'static str,
        cleanup: Result<(), CleanupError>,
        diagnostics: Result<(), String>,
    ) -> Result<(), CleanupError> {
        match (cleanup, diagnostics) {
            (Ok(()), Ok(())) => {
                self.cleanup_complete = true;
                Ok(())
            }
            (Err(error), Ok(())) => {
                self.cleanup_failure_observed = true;
                Err(error)
            }
            (Ok(()), Err(error)) => {
                self.cleanup_failure_observed = true;
                Err(CleanupError {
                    kind: self.kind.cli_value(),
                    pid: self.child.id(),
                    phase,
                    state: ExitState::Reaped,
                    failure: CleanupFailure::Diagnostic(error),
                })
            }
            (Err(error), Err(diagnostic)) => {
                self.cleanup_failure_observed = true;
                Err(combine_cleanup_error(error, diagnostic))
            }
        }
    }

    fn join_diagnostic_readers(&mut self) -> Result<(), String> {
        let readers = std::mem::take(&mut self.diagnostic_readers);
        let mut pending = Vec::new();
        let mut first_error = None;
        for mut reader in readers {
            reader.cancel();
            if let Err(error) = reader.join_bounded(DIAGNOSTIC_JOIN_TIMEOUT) {
                if first_error.is_none() {
                    first_error = Some(error);
                }
                pending.push(reader);
            }
        }
        self.diagnostic_readers = pending;
        first_error.map_or(Ok(()), Err)
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.cleanup_complete {
            return;
        }
        let failure_was_already_observed = self.cleanup_failure_observed;
        if let Err(error) = self.force_cleanup("panic") {
            if thread::panicking() || failure_was_already_observed {
                eprintln!("generated process cleanup failed during drop: {error}");
            } else {
                panic!("{error}");
            }
        }
    }
}

struct DiagnosticReader {
    receiver: mpsc::Receiver<String>,
    join_handle: Option<thread::JoinHandle<()>>,
    cancel_sender: mpsc::SyncSender<()>,
    join_error: Option<String>,
}

impl DiagnosticReader {
    fn cancel(&self) {
        let _ = self.cancel_sender.try_send(());
    }

    fn join_bounded(&mut self, timeout: Duration) -> Result<(), String> {
        if let Some(error) = &self.join_error {
            return Err(error.clone());
        }

        let Some(handle) = self.join_handle.as_ref() else {
            return Ok(());
        };
        let deadline = Instant::now() + timeout;
        while !handle.is_finished() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(format!(
                    "diagnostic reader did not terminate within {timeout:?}"
                ));
            }
            thread::sleep(std::cmp::min(remaining, Duration::from_millis(10)));
        }

        let handle = self
            .join_handle
            .take()
            .expect("diagnostic reader join handle must be present");
        match handle.join() {
            Ok(()) => Ok(()),
            Err(_) => {
                let error = "diagnostic reader thread panicked".to_owned();
                self.join_error = Some(error.clone());
                Err(error)
            }
        }
    }
}

fn spawn_bounded_diagnostic<R>(stream: R) -> DiagnosticReader
where
    R: Read + Send + 'static,
{
    let (sender, receiver) = mpsc::sync_channel(1);
    let (cancel_sender, cancel_receiver) = mpsc::sync_channel(1);
    let join_handle = thread::spawn(move || {
        let mut stream = stream.take(MAX_STARTUP_DIAGNOSTIC_BYTES);
        let mut output = Vec::new();
        let diagnostic = loop {
            match cancel_receiver.try_recv() {
                Ok(()) | Err(TryRecvError::Disconnected) => {
                    break "<output read cancelled>".to_owned();
                }
                Err(TryRecvError::Empty) => {}
            }

            let mut buffer = [0_u8; 4096];
            match stream.read(&mut buffer) {
                Ok(0) if output.is_empty() => break "<no output>".to_owned(),
                Ok(0) => break text(&output),
                Ok(read) => output.extend_from_slice(&buffer[..read]),
                Err(error) => break format!("<could not read output: {error}>"),
            }
        };
        let _ = sender.send(diagnostic);
    });

    DiagnosticReader {
        receiver,
        join_handle: Some(join_handle),
        cancel_sender,
        join_error: None,
    }
}

fn collect_bounded_diagnostic(reader: &mut DiagnosticReader) -> (String, bool) {
    match reader.receiver.recv_timeout(STARTUP_DIAGNOSTIC_TIMEOUT) {
        Ok(diagnostic) => {
            let reader_pending = reader.join_bounded(DIAGNOSTIC_JOIN_TIMEOUT).is_err();
            (diagnostic, reader_pending)
        }
        Err(RecvTimeoutError::Timeout) => {
            reader.cancel();
            (
                format!(
                    "<output read timed out after {:?}>",
                    STARTUP_DIAGNOSTIC_TIMEOUT
                ),
                true,
            )
        }
        Err(RecvTimeoutError::Disconnected) => {
            let reader_pending = reader.join_bounded(DIAGNOSTIC_JOIN_TIMEOUT).is_err();
            (
                "<output reader stopped unexpectedly>".to_owned(),
                reader_pending,
            )
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
enum ExitState {
    Reaped,
    Running,
    Unknown,
}

impl fmt::Display for ExitState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = match self {
            Self::Reaped => "reaped",
            Self::Running => "running",
            Self::Unknown => "unknown",
        };
        formatter.write_str(state)
    }
}

#[derive(Debug, Eq, PartialEq)]
enum CleanupFailure {
    Inspection(String),
    Kill(String),
    Timeout(Duration),
    Diagnostic(String),
    Combined {
        process: Box<CleanupFailure>,
        diagnostic: String,
    },
}

impl fmt::Display for CleanupFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inspection(error) => write!(formatter, "inspection error: {error}"),
            Self::Kill(error) => write!(formatter, "force-kill error: {error}"),
            Self::Timeout(timeout) => {
                write!(formatter, "cleanup deadline expired after {timeout:?}")
            }
            Self::Diagnostic(error) => write!(formatter, "diagnostic reader error: {error}"),
            Self::Combined {
                process,
                diagnostic,
            } => write!(
                formatter,
                "process cleanup error: {process}; diagnostic reader error: {diagnostic}"
            ),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
struct CleanupError {
    kind: &'static str,
    pid: u32,
    phase: &'static str,
    state: ExitState,
    failure: CleanupFailure,
}

fn combine_cleanup_error(mut error: CleanupError, diagnostic: String) -> CleanupError {
    error.failure = CleanupFailure::Combined {
        process: Box::new(error.failure),
        diagnostic,
    };
    error
}

impl fmt::Display for CleanupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} (pid {}) {} cleanup failed: {}; exit state: {}",
            self.kind, self.pid, self.phase, self.failure, self.state
        )
    }
}

trait CleanupProcess {
    fn cleanup_pid(&self) -> u32;
    fn cleanup_try_wait(&mut self) -> io::Result<Option<()>>;
    fn cleanup_kill(&mut self) -> io::Result<()>;
}

impl CleanupProcess for Child {
    fn cleanup_pid(&self) -> u32 {
        self.id()
    }

    fn cleanup_try_wait(&mut self) -> io::Result<Option<()>> {
        self.try_wait().map(|status| status.map(|_| ()))
    }

    fn cleanup_kill(&mut self) -> io::Result<()> {
        self.kill()
    }
}

fn cleanup_process<P: CleanupProcess>(
    process: &mut P,
    kind: &'static str,
    phase: &'static str,
    timeout: Duration,
) -> Result<(), CleanupError> {
    let pid = process.cleanup_pid();
    let initial_inspection = match process.cleanup_try_wait() {
        Ok(Some(_)) => return Ok(()),
        Ok(None) => None,
        Err(error) => Some(error),
    };

    let kill_error = process.cleanup_kill().err();
    let reaping = poll_for_cleanup(process, timeout);
    let state = match &reaping {
        Ok(Some(_)) => ExitState::Reaped,
        Ok(None) => ExitState::Running,
        Err(_) => ExitState::Unknown,
    };

    if let Some(error) = initial_inspection {
        let detail = match &reaping {
            Ok(_) => error.to_string(),
            Err(reaping_error) => {
                format!("{error}; reaping inspection error: {reaping_error}")
            }
        };
        return Err(CleanupError {
            kind,
            pid,
            phase,
            state,
            failure: CleanupFailure::Inspection(detail),
        });
    }

    if let Some(error) = kill_error {
        let detail = match &reaping {
            Ok(_) => error.to_string(),
            Err(reaping_error) => format!("{error}; reaping inspection error: {reaping_error}"),
        };
        return Err(CleanupError {
            kind,
            pid,
            phase,
            state,
            failure: CleanupFailure::Kill(detail),
        });
    }

    match reaping {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(CleanupError {
            kind,
            pid,
            phase,
            state,
            failure: CleanupFailure::Timeout(timeout),
        }),
        Err(error) => Err(CleanupError {
            kind,
            pid,
            phase,
            state,
            failure: CleanupFailure::Inspection(format!("reaping inspection error: {error}")),
        }),
    }
}

fn poll_for_cleanup<P: CleanupProcess>(
    process: &mut P,
    timeout: Duration,
) -> io::Result<Option<()>> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = process.cleanup_try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        thread::sleep(Duration::from_millis(50));
    }
}

struct ShutdownOutcome {
    status: ExitStatus,
    graceful: bool,
    forced: bool,
}

fn terminate(child: &mut Child, kind: Kind) -> ShutdownOutcome {
    if let Some(status) = child
        .try_wait()
        .unwrap_or_else(|error| panic!("could not inspect {} process: {error}", kind.cli_value()))
    {
        return ShutdownOutcome {
            status,
            graceful: false,
            forced: false,
        };
    }

    let pid = child.id();
    let signal = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()
        .unwrap_or_else(|error| {
            panic!(
                "could not request graceful shutdown for {} (pid {pid}): {error}",
                kind.cli_value()
            )
        });
    assert!(
        signal.success(),
        "could not request graceful shutdown for {} (pid {pid}); kill exited with {signal}",
        kind.cli_value()
    );

    if let Some(status) = wait_for_exit(child, GRACEFUL_SHUTDOWN_TIMEOUT, kind) {
        return ShutdownOutcome {
            status,
            graceful: true,
            forced: false,
        };
    }

    let force_error = child.kill().err();
    if let Some(error) = force_error {
        panic!(
            "{} (pid {pid}) did not exit within {:?} after SIGTERM and could not be force-killed: {error}",
            kind.cli_value(),
            GRACEFUL_SHUTDOWN_TIMEOUT
        );
    }

    if let Some(status) = wait_for_exit(child, FORCEFUL_SHUTDOWN_TIMEOUT, kind) {
        return ShutdownOutcome {
            status,
            graceful: false,
            forced: true,
        };
    }

    panic!(
        "{} (pid {pid}) remained running after {:?} graceful and {:?} forceful shutdown deadlines",
        kind.cli_value(),
        GRACEFUL_SHUTDOWN_TIMEOUT,
        FORCEFUL_SHUTDOWN_TIMEOUT
    );
}

fn terminate_with_success(child: &mut Child, kind: Kind) {
    let outcome = terminate(child, kind);
    assert!(
        outcome.graceful,
        "{} did not terminate through the graceful SIGTERM path within {:?}; forced={}, exit status was {:?}",
        kind.cli_value(),
        GRACEFUL_SHUTDOWN_TIMEOUT,
        outcome.forced,
        outcome.status
    );
    assert!(
        outcome.status.success(),
        "{} terminated unsuccessfully: {:?}",
        kind.cli_value(),
        outcome.status
    );
}

fn wait_for_exit(child: &mut Child, timeout: Duration, kind: Kind) -> Option<ExitStatus> {
    match poll_for_exit(child, timeout) {
        Ok(status) => status,
        Err(error) => panic!("could not inspect {} process: {error}", kind.cli_value()),
    }
}

fn poll_for_exit(child: &mut Child, timeout: Duration) -> io::Result<Option<ExitStatus>> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn run_cli(arguments: &[&str]) {
    run_cli_in(Path::new("."), arguments);
}

fn run_cli_in(directory: &Path, arguments: &[&str]) {
    let output = Command::new(env!("CARGO_BIN_EXE_ember"))
        .args(arguments)
        .current_dir(directory)
        .output()
        .unwrap_or_else(|error| panic!("could not start ember CLI: {error}"));
    assert_success("ember CLI", arguments, &output);
}

fn run_cli_failure(arguments: &[&str]) -> Output {
    let output = Command::new(env!("CARGO_BIN_EXE_ember"))
        .args(arguments)
        .output()
        .unwrap_or_else(|error| panic!("could not start ember CLI: {error}"));
    assert!(
        !output.status.success(),
        "expected CLI failure for {arguments:?}"
    );
    output
}

fn assert_success(command: &str, arguments: &[&str], output: &Output) {
    assert!(
        output.status.success(),
        "{command} failed for {arguments:?}:\n{}\n{}",
        text(&output.stdout),
        text(&output.stderr)
    );
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

struct TestWorkspace {
    root: PathBuf,
}

impl TestWorkspace {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = env::temp_dir().join(format!("ember-cli-{label}-{}-{nonce}", process_id()));
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn process_id() -> u32 {
    std::process::id()
}

struct RemoveOnDrop(PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        if self.0.exists() {
            fs::remove_dir_all(&self.0)
                .unwrap_or_else(|error| panic!("could not clean up {}: {error}", self.0.display()));
        }
    }
}
