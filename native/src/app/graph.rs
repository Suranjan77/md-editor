//! The knowledge graph: snapshot loading, filters, selection and the physics layout.

use super::*;
use crate::graph_state::{GraphScope, GraphState};
use crate::messages::{GraphMessage, VaultMessage};

impl MdEditor {
    /// Handle a message routed here by [`MdEditor::update_inner`].
    pub(super) fn update_graph(&mut self, message: GraphMessage) -> Task<Message> {
        match message {
            GraphMessage::Toggle => {
                self.on_graph_toggle();
                Task::none()
            }
            GraphMessage::Refresh => self.on_graph_refresh(),
            GraphMessage::SnapshotLoaded(generation, result) => {
                match result {
                    Ok(snapshot) => {
                        self.graph.complete_refresh(generation, snapshot);
                    }
                    Err(error) => {
                        if self.graph.fail_refresh(generation) && self.graph.visible {
                            self.ui.toast =
                                Some(format!("Could not refresh knowledge graph: {error}"));
                        }
                    }
                }
                Task::none()
            }
            GraphMessage::NodeOpen(path) => {
                let exists = self.graph.node(&path).is_some_and(|node| node.exists);
                if exists {
                    // The graph is a full-window workspace, so leaving it up
                    // would hide the document the user just asked to read.
                    self.graph.visible = false;
                    Task::done(Message::Vault(VaultMessage::SidebarFileClicked(path)))
                } else {
                    self.ui.toast = Some("That link target does not exist yet".to_string());
                    Task::none()
                }
            }
            m => {
                apply_graph_view(&mut self.graph, m);
                Task::none()
            }
        }
    }

    fn on_graph_toggle(&mut self) {
        if self.vault.root.is_none() {
            return;
        }
        if self.graph.visible {
            self.graph.visible = false;
        } else {
            self.graph.visible = true;
            self.tracker.hide();
            self.search.visible = false;
            self.search.file_visible = false;
            self.ui.command_palette_visible = false;
            self.graph.retry_refresh();
            let active = self.graph.active_path.clone();
            self.graph.select(active);
        }
    }

    fn on_graph_refresh(&mut self) -> Task<Message> {
        if !self.graph.visible {
            return Task::none();
        }
        let Some(generation) = self.graph.begin_refresh() else {
            return Task::none();
        };
        let state = self.state.clone();
        Task::perform(
            async move { md_editor_core::vault::get_graph_snapshot(&state).map_err(String::from) },
            move |result| Message::Graph(GraphMessage::SnapshotLoaded(generation, result)),
        )
    }

    pub(super) fn invalidate_graph(&mut self) {
        self.graph.mark_dirty();
    }
}

/// Graph messages that only change the graph's own view state: filters,
/// selection, pins, camera and physics.
fn apply_graph_view(graph: &mut GraphState, message: GraphMessage) {
    match message {
        GraphMessage::ScopeGlobal => graph.set_scope(GraphScope::Global),
        GraphMessage::ScopeLocal => {
            let depth = match graph.scope {
                GraphScope::Local { depth } => depth,
                GraphScope::Global => 1,
            };
            graph.set_scope(GraphScope::Local { depth });
        }
        GraphMessage::LocalDepthChanged(depth) => graph.set_scope(GraphScope::Local { depth }),
        GraphMessage::QueryChanged(query) => graph.set_query(query),
        GraphMessage::ShowPdfsToggled(show) => graph.set_show_pdfs(show),
        GraphMessage::ShowMissingToggled(show) => graph.set_show_missing(show),
        GraphMessage::ShowOrphansToggled(show) => graph.set_show_orphans(show),
        GraphMessage::NodeSelected(path) => graph.select(path),
        GraphMessage::NodeMoved { path, x, y } => graph.move_node(&path, x, y),
        GraphMessage::NodeFocused(path) => graph.focus_on(&path),
        GraphMessage::PinToggled(path) => graph.toggle_pin(&path),
        GraphMessage::UnpinAll => graph.unpin_all(),
        GraphMessage::ZoomBy(factor) => graph.zoom_by(factor),
        GraphMessage::PhysicsToggled => graph.toggle_physics(),
        GraphMessage::FiltersReset => graph.reset_filters(),
        GraphMessage::InspectorToggled => graph.inspector_visible = !graph.inspector_visible,
        GraphMessage::FitView => graph.fit_revision = graph.fit_revision.wrapping_add(1),
        GraphMessage::ResetLayout => graph.reset_layout(),
        GraphMessage::PhysicsTick => {
            graph.physics_tick();
        }
        GraphMessage::SetView { pan_x, pan_y, zoom } => graph.set_view(pan_x, pan_y, zoom),
        GraphMessage::Hovered(path) => graph.set_hovered(path),
        // Handled by `update_graph`; listed so a new variant must be routed.
        GraphMessage::Toggle
        | GraphMessage::Refresh
        | GraphMessage::SnapshotLoaded(..)
        | GraphMessage::NodeOpen(_) => {}
    }
}
