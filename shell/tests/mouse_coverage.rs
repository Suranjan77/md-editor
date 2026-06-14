use std::collections::BTreeSet;

use md3_kernel::CommandId;
use md3_kernel::defaults::default_registry;
use md3_kernel::input::EditorKind;
use md3_shell::gui::menu;

const MOUSE_EXEMPT: [CommandId; 11] = [
    CommandId("overlay.close"),
    CommandId("overlay.confirm"),
    CommandId("app.settings"),
    CommandId("app.force-quit"),
    CommandId("workspace.force-close-tab"),
    CommandId("editor.heading-1"),
    CommandId("editor.heading-2"),
    CommandId("editor.heading-3"),
    CommandId("editor.heading-4"),
    CommandId("editor.heading-5"),
    CommandId("editor.heading-6"),
];

#[test]
fn every_registered_command_is_mouse_reachable_or_explicitly_exempt() {
    let registry = match default_registry() {
        Ok(registry) => registry,
        Err(e) => panic!("registry: {e}"),
    };
    let mut reachable = BTreeSet::new();
    for kind in [None, Some(EditorKind::Markdown), Some(EditorKind::Pdf)] {
        for item in menu::menu_model(&registry, kind, true)
            .into_iter()
            .flat_map(|group| group.items)
        {
            reachable.insert(item.command);
        }
    }
    reachable.extend(MOUSE_EXEMPT);

    let missing: Vec<&str> = registry
        .specs()
        .filter(|spec| !reachable.contains(&spec.id))
        .map(|spec| spec.id.0)
        .collect();
    assert!(
        missing.is_empty(),
        "commands missing mouse placement: {missing:?}"
    );
}
