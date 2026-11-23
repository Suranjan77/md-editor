//! The label overlay: node labels and the hover card, drawn on a canvas.

use super::*;

// ─────────────────────────── label overlay ──────────────────────────

/// The transparent middle layer. Renders node labels, pin markers, and the
/// hover card with Iced's text pipeline (crisp, unlike text baked into the
/// shader), and is deliberately inert: it captures no events and reports
/// `Interaction::None`, so the GPU layer underneath receives every pointer
/// gesture with a live cursor.
pub(crate) struct GraphLabels {
    pub(super) frame: GraphFrame,
}

impl GraphLabels {
    pub(crate) fn new(frame: GraphFrame) -> Self {
        Self { frame }
    }
}

impl canvas::Program<Message> for GraphLabels {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        _event: &canvas::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        None
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        if self.frame.nodes().is_empty() {
            // The empty-state card is a real widget layer above this one.
            return vec![frame.into_geometry()];
        }

        let transform = ViewTransform {
            pan: self.frame.pan,
            zoom: self.frame.zoom,
        };
        let focus_mask = self.frame.focus_mask();
        let focus = focus_mask.as_ref().map(|(focus, _)| *focus);
        let in_focus = |index: usize| {
            focus_mask
                .as_ref()
                .is_some_and(|(_, mask)| mask.get(index).copied().unwrap_or(false))
        };
        let degree_cutoff = label_degree_cutoff(&self.frame.graph.snapshot, transform.zoom);

        for (index, node) in self.frame.nodes().iter().enumerate() {
            let position = world_to_screen(self.frame.world(index), bounds, transform);
            let radius = node_screen_radius(node, transform.zoom);
            // Generous margin so a label anchored just off-screen still draws.
            if !circle_may_be_visible(position, radius + 140.0, bounds) {
                continue;
            }

            let selected = self.frame.selected == Some(index);
            let hovered = self.frame.hovered == Some(index);
            let active = self.frame.active == Some(index);
            let is_focus = focus == Some(index);
            let neighbor = !is_focus && in_focus(index);

            // A pin is a promise the node will stay put, so it needs to be
            // visible without hovering or selecting anything.
            if self.frame.pinned.contains(&index) {
                frame.stroke(
                    &canvas::Path::circle(position, radius + 4.0),
                    canvas::Stroke {
                        line_dash: canvas::LineDash {
                            segments: &[2.0, 3.0],
                            offset: 0,
                        },
                        ..canvas::Stroke::default()
                            .with_color(Color {
                                a: 0.75,
                                ..theme::ACCENT
                            })
                            .with_width(1.0)
                    },
                );
            }

            let important = (node.incoming + node.outgoing) >= degree_cutoff;
            if !(hovered || selected || active || neighbor || is_focus || important) {
                continue;
            }

            let emphasized = selected || hovered || is_focus;
            let faded = focus.is_some() && !emphasized && !neighbor;
            frame.fill_text(canvas::Text {
                content: truncate_label(&node.label, if emphasized { 48 } else { 26 }),
                position: Point::new(position.x, position.y + radius + 4.0),
                color: if faded {
                    Color {
                        a: 0.45,
                        ..theme::TEXT_MUTED
                    }
                } else if emphasized {
                    theme::TEXT_PRIMARY
                } else {
                    theme::TEXT_SECONDARY
                },
                size: iced::Pixels(if emphasized { 12.0 } else { 10.0 }),
                align_x: iced::advanced::text::Alignment::Center,
                align_y: iced::alignment::Vertical::Top,
                ..canvas::Text::default()
            });
        }

        if let (Some(index), Some(cursor)) = (self.frame.hovered, cursor.position_in(bounds))
            && let Some(node) = self.frame.nodes().get(index)
        {
            draw_hover_card(
                &mut frame,
                node,
                self.frame.pinned.contains(&index),
                cursor,
                bounds,
            );
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        // Report `None` so the stack does not levitate the cursor away from the
        // GPU layer below, which is what actually handles interaction.
        mouse::Interaction::None
    }
}

/// The minimum degree a node needs for its label to be drawn unconditionally.
///
/// Labelling every node in a large vault produces an unreadable smear and makes
/// text shaping the frame budget, so the budget grows with zoom: zoomed out you
/// see the hubs, zoomed in you see everything nearby.
pub(super) fn label_degree_cutoff(graph: &GraphSnapshot, zoom: f32) -> usize {
    if graph.nodes.len() <= LABEL_ALL_BELOW || zoom >= 1.3 {
        return 0;
    }
    let budget = (graph.nodes.len() as f32 * (0.10 + 0.5 * (zoom - 0.3).max(0.0)))
        .round()
        .clamp(12.0, 160.0) as usize;
    if budget >= graph.nodes.len() {
        return 0;
    }
    let mut degrees: Vec<usize> = graph
        .nodes
        .iter()
        .map(|node| node.incoming + node.outgoing)
        .collect();
    degrees.sort_unstable_by(|a, b| b.cmp(a));
    degrees[budget.saturating_sub(1)].max(1)
}

pub(super) fn truncate_label(label: &str, limit: usize) -> String {
    if label.chars().count() <= limit {
        return label.to_string();
    }
    let mut out: String = label.chars().take(limit.saturating_sub(1)).collect();
    out.push('…');
    out
}
