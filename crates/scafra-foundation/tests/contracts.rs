use scafra_foundation::{
    phase::{Phase, PhaseMetadata},
    shutdown::{ShutdownPolicy, ShutdownReason},
};
use std::time::Duration;

#[test]
fn phases_have_stable_discriminants_names_and_order() {
    let phases = Phase::ALL;
    let expected = [
        (Phase::CompileTime, 0, "compile_time"),
        (Phase::BuildTime, 1, "build_time"),
        (Phase::Startup, 2, "startup"),
        (Phase::Runtime, 3, "runtime"),
        (Phase::Shutdown, 4, "shutdown"),
    ];

    assert_eq!(phases, expected.map(|(phase, _, _)| phase));
    for (phase, order, name) in expected {
        assert_eq!(phase as u8, order);
        assert_eq!(phase.order(), order);
        assert_eq!(phase.name(), name);
    }
}

#[test]
fn phase_metadata_preserves_static_values_and_extension_order() {
    const METADATA: PhaseMetadata =
        PhaseMetadata::new(Phase::Runtime, "http_server", 12, "scafra-web::server");

    assert_eq!(METADATA.phase(), Phase::Runtime);
    assert_eq!(METADATA.name(), "http_server");
    assert_eq!(METADATA.order(), 12);
    assert_eq!(METADATA.source(), "scafra-web::server");
    assert_eq!(METADATA, METADATA);
}

#[test]
fn shutdown_reasons_are_exhaustive_contract_values() {
    let reasons = [
        ShutdownReason::Signal,
        ShutdownReason::ApplicationRequest,
        ShutdownReason::StartupFailure,
        ShutdownReason::RuntimeFailure,
    ];

    assert_eq!(reasons.len(), 4);
    assert_eq!(reasons[0], ShutdownReason::Signal);
    assert_eq!(reasons[1], ShutdownReason::ApplicationRequest);
    assert_eq!(reasons[2], ShutdownReason::StartupFailure);
    assert_eq!(reasons[3], ShutdownReason::RuntimeFailure);
}

#[test]
fn shutdown_policy_defaults_to_bounded_graceful_cleanup() {
    let policy = ShutdownPolicy::default();

    assert_eq!(policy.grace_period(), Duration::from_secs(30));
    assert_eq!(policy.grace_period(), ShutdownPolicy::DEFAULT_GRACE_PERIOD);
    assert!(policy.force_after_grace());
}

#[test]
fn shutdown_policy_accepts_an_explicit_zero_duration() {
    let policy = ShutdownPolicy::new(Duration::ZERO, false);

    assert_eq!(policy.grace_period(), Duration::ZERO);
    assert!(!policy.force_after_grace());
}
