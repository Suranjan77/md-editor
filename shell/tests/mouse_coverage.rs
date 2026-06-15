use md_kernel::defaults::default_registry;

/// With the menu bar gone (Quiet Vault), the command palette is the universal
/// mouse surface: the command spine's ⌘K button (a `Message::RunCommand`
/// click) opens it, and every registered command is then one click away in the
/// palette list. Guard that invariant — `palette("")` must enumerate every
/// command, so none is mouse-orphaned.
#[test]
fn every_registered_command_is_reachable_through_the_palette() {
    let registry = match default_registry() {
        Ok(registry) => registry,
        Err(e) => panic!("registry: {e}"),
    };

    let listed: std::collections::BTreeSet<_> = registry
        .palette("")
        .into_iter()
        .map(|spec| spec.id)
        .collect();

    let missing: Vec<&str> = registry
        .specs()
        .filter(|spec| !listed.contains(&spec.id))
        .map(|spec| spec.id.0)
        .collect();
    assert!(
        missing.is_empty(),
        "commands missing from the palette (mouse-unreachable): {missing:?}"
    );
}
