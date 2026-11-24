//! The knowledge graph: snapshot loading, filters, selection and the physics layout.

use super::*;
use crate::messages::{GraphMessage, VaultMessage};

impl MdEditor {
    /// Handle a message routed here by [`MdEditor::update_inner`].
    pub(super) fn update_graph(&mut self, message: GraphMessage) -> Task<Message> {
        match message {
            GraphMessage::Toggle => {
                if self.vault.root.is_none() {
                    return Task::none();
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
                Task::none()
            }
            GraphMessage::Refresh => {
                if !self.graph.visible {
                    return Task::none();
                }
                let Some(generation) = self.graph.begin_refresh() else {
                    return Task::none();
                };
                let state = self.state.clone();
                Task::perform(
                    async move { md_editor_core::vault::get_graph_snapshot(&state) },
                    move |result| Message::Graph(GraphMessage::SnapshotLoaded(generation, result)),
                )
            }
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
            GraphMessage::ScopeGlobal => {
                self.graph.set_scope(crate::graph_state::GraphScope::Global);
                Task::none()
            }
            GraphMessage::ScopeLocal => {
                let depth = match self.graph.scope {
                    crate::graph_state::GraphScope::Local { depth } => depth,
                    crate::graph_state::GraphScope::Global => 1,
                };
                self.graph
                    .set_scope(crate::graph_state::GraphScope::Local { depth });
                Task::none()
            }
            GraphMessage::LocalDepthChanged(depth) => {
                self.graph
                    .set_scope(crate::graph_state::GraphScope::Local { depth });
                Task::none()
            }
            GraphMessage::QueryChanged(query) => {
                self.graph.set_query(query);
                Task::none()
            }
            GraphMessage::ShowPdfsToggled(show) => {
                self.graph.set_show_pdfs(show);
                Task::none()
            }
            GraphMessage::ShowMissingToggled(show) => {
                self.graph.set_show_missing(show);
                Task::none()
            }
            GraphMessage::ShowOrphansToggled(show) => {
                self.graph.set_show_orphans(show);
                Task::none()
            }
            GraphMessage::NodeSelected(path) => {
                self.graph.select(path);
                Task::none()
            }
            GraphMessage::NodeMoved { path, x, y } => {
                self.graph.move_node(&path, x, y);
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
            GraphMessage::NodeFocused(path) => {
                self.graph.focus_on(&path);
                Task::none()
            }
            GraphMessage::PinToggled(path) => {
                self.graph.toggle_pin(&path);
                Task::none()
            }
            GraphMessage::UnpinAll => {
                self.graph.unpin_all();
                Task::none()
            }
            GraphMessage::ZoomBy(factor) => {
                self.graph.zoom_by(factor);
                Task::none()
            }
            GraphMessage::PhysicsToggled => {
                self.graph.toggle_physics();
                Task::none()
            }
            GraphMessage::FiltersReset => {
                self.graph.reset_filters();
                Task::none()
            }
            GraphMessage::InspectorToggled => {
                self.graph.inspector_visible = !self.graph.inspector_visible;
                Task::none()
            }
            GraphMessage::FitView => {
                self.graph.fit_revision = self.graph.fit_revision.wrapping_add(1);
                Task::none()
            }
            GraphMessage::ResetLayout => {
                self.graph.reset_layout();
                Task::none()
            }
            GraphMessage::PhysicsTick => {
                self.graph.physics_tick();
                Task::none()
            }
            GraphMessage::SetView { pan_x, pan_y, zoom } => {
                self.graph.set_view(pan_x, pan_y, zoom);
                Task::none()
            }
            GraphMessage::Hovered(path) => {
                self.graph.set_hovered(path);
                Task::none()
            }
        }
    }

    pub(super) fn invalidate_graph(&mut self) {
        self.graph.mark_dirty();
    }
}
