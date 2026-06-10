//! Integration-test scaffolding: asserts the core crate links and its
//! top-level API surface is reachable from an external crate.

#[test]
fn core_crate_links_and_state_initializes() {
    let state = md_editor_core::AppState::try_new_in_memory()
        .expect("in-memory application state should initialize");
    drop(state);
}
