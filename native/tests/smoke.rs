//! Integration-test scaffolding for the native crate.
//!
//! `md-editor-native` is a binary-only crate (no lib target), so external
//! tests cannot import its modules; unit/characterization tests live in-crate
//! under `#[cfg(test)]`. This smoke test still forces Cargo to build the
//! binary and asserts the artifact exists, which catches link failures.

#[test]
fn native_binary_builds_and_links() {
    let exe = env!("CARGO_BIN_EXE_md-editor");
    assert!(
        std::path::Path::new(exe).exists(),
        "built binary missing at {exe}"
    );
}
