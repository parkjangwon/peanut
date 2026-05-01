use std::fs;

#[test]
fn compose_verifier_covers_app_isolation_and_restore_gate() {
    let script = fs::read_to_string("scripts/verify-compose.sh").unwrap();

    for marker in [
        "Verifying app A/B isolation",
        "Verifying cross-app Data denial",
        "Verifying cross-app Storage denial",
        "Verifying cross-app Function denial",
        "Verifying disabled app blocks and re-enables SDK access",
        "Verifying restore-pending clear keeps readiness clean",
        "All compose production gate checks passed",
    ] {
        assert!(
            script.contains(marker),
            "scripts/verify-compose.sh must contain production gate marker: {marker}"
        );
    }
}

#[test]
fn ci_requires_full_compose_gate_before_release() {
    let smoke = fs::read_to_string(".github/workflows/smoke.yml").unwrap();
    let release = fs::read_to_string(".github/workflows/release.yml").unwrap();

    assert!(smoke.contains("compose-gate:"));
    assert!(smoke.contains("scripts/verify-compose.sh"));
    assert!(smoke.contains("Upload compose gate artifacts"));
    assert!(release.contains("Run released image production gate"));
    assert!(release.contains("scripts/check-openapi.sh"));
    assert!(release.contains("Backup contract check"));
}
