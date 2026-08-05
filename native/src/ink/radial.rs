//! A menu that comes to the pen instead of making the pen come to it.
//!
//! On an opaque tablet your eyes are on the screen while your hand works
//! blind, so every trip to a toolbar costs the writing thread twice: once to
//! travel, once to reacquire the nib. A ring summoned at the nib has zero
//! travel, and direction is far easier to learn blind than position — you end
//! up flicking north-east for the highlighter without looking.
//!
//! Two rings: tools near the centre, colours further out. Distance picks the
//! ring, angle picks the slot, and the middle is a dead zone that cancels.

use iced::Color;

use super::Tool;
use super::stroke::Vec2;

/// Inside this radius nothing is selected, so opening the menu and lifting
/// without moving is a no-op rather than a surprise.
pub const DEAD_ZONE: f32 = 26.0;

/// Boundary between the tool ring and the colour ring.
pub const RING_SPLIT: f32 = 78.0;

/// Beyond this the gesture is treated as abandoned.
pub const OUTER_LIMIT: f32 = 150.0;

/// Where the tool labels sit.
pub const TOOL_RADIUS: f32 = 52.0;

/// Where the colour dots sit.
pub const COLOR_RADIUS: f32 = 112.0;

/// Tools in ring order, starting at the top and going clockwise.
pub const TOOLS: [(&str, Tool); 4] = [
    ("Pen", Tool::Pen),
    ("Mark", Tool::Highlighter),
    ("Erase", Tool::Eraser),
    ("Select", Tool::Lasso),
];

/// What lifting the pen at a given position would do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RadialAction {
    Tool(Tool),
    Color(Color),
}

/// An open radial menu, anchored where the pen summoned it.
#[derive(Debug, Clone, Copy)]
pub struct RadialMenu {
    /// Centre, in surface-local screen pixels.
    pub center: Vec2,
    /// Where the pen is now, for highlighting the slot under it.
    pub cursor: Vec2,
}

impl RadialMenu {
    pub fn new(center: Vec2) -> Self {
        Self {
            center,
            cursor: center,
        }
    }

    /// The slot currently under the pen, if any.
    pub fn highlighted(&self) -> Option<RadialAction> {
        action_at(self.center, self.cursor)
    }
}

/// Angle of `point` about `center`, measured clockwise from straight up so
/// slot 0 sits at the top.
fn slot_angle(center: Vec2, point: Vec2) -> f32 {
    let dx = point.x - center.x;
    let dy = point.y - center.y;
    // atan2(dx, -dy) puts 0 at the top and grows clockwise.
    let angle = dx.atan2(-dy);
    if angle < 0.0 {
        angle + std::f32::consts::TAU
    } else {
        angle
    }
}

/// Which of `count` evenly spaced slots an angle falls in, with slot centres
/// aligned to the top.
fn slot_index(angle: f32, count: usize) -> usize {
    let step = std::f32::consts::TAU / count as f32;
    // Offset by half a slot so the first slot straddles straight up.
    let shifted = (angle + step * 0.5).rem_euclid(std::f32::consts::TAU);
    ((shifted / step) as usize).min(count - 1)
}

/// Resolve a pen position into the action it would commit.
pub fn action_at(center: Vec2, cursor: Vec2) -> Option<RadialAction> {
    let distance = cursor.dist(center);
    if distance < DEAD_ZONE || distance > OUTER_LIMIT {
        return None;
    }

    let angle = slot_angle(center, cursor);

    if distance < RING_SPLIT {
        let index = slot_index(angle, TOOLS.len());
        Some(RadialAction::Tool(TOOLS[index].1))
    } else {
        let index = slot_index(angle, super::COLOR_PRESETS.len());
        Some(RadialAction::Color(super::COLOR_PRESETS[index].1))
    }
}

/// Screen position of a slot's label or dot.
pub fn slot_position(center: Vec2, index: usize, count: usize, radius: f32) -> Vec2 {
    let step = std::f32::consts::TAU / count as f32;
    let angle = step * index as f32;
    Vec2::new(
        center.x + radius * angle.sin(),
        center.y - radius * angle.cos(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const CENTER: Vec2 = Vec2::new(400.0, 300.0);

    fn at(dx: f32, dy: f32) -> Vec2 {
        Vec2::new(CENTER.x + dx, CENTER.y + dy)
    }

    #[test]
    fn the_middle_is_a_dead_zone() {
        assert_eq!(action_at(CENTER, CENTER), None);
        assert_eq!(action_at(CENTER, at(10.0, 10.0)), None);
    }

    #[test]
    fn flicking_far_out_abandons_the_gesture() {
        assert_eq!(action_at(CENTER, at(0.0, -400.0)), None);
    }

    #[test]
    fn straight_up_selects_the_first_tool() {
        assert_eq!(
            action_at(CENTER, at(0.0, -TOOL_RADIUS)),
            Some(RadialAction::Tool(Tool::Pen))
        );
    }

    #[test]
    fn the_tool_ring_runs_clockwise_from_the_top() {
        // Four tools: up, right, down, left.
        let expected = [
            (0.0, -TOOL_RADIUS, Tool::Pen),
            (TOOL_RADIUS, 0.0, Tool::Highlighter),
            (0.0, TOOL_RADIUS, Tool::Eraser),
            (-TOOL_RADIUS, 0.0, Tool::Lasso),
        ];
        for (dx, dy, tool) in expected {
            assert_eq!(
                action_at(CENTER, at(dx, dy)),
                Some(RadialAction::Tool(tool)),
                "direction ({dx}, {dy}) should pick {tool:?}"
            );
        }
    }

    #[test]
    fn the_outer_ring_picks_colours() {
        let action = action_at(CENTER, at(0.0, -COLOR_RADIUS));
        assert_eq!(
            action,
            Some(RadialAction::Color(super::super::COLOR_PRESETS[0].1))
        );
    }

    #[test]
    fn distance_alone_decides_which_ring() {
        // Same direction, two distances, two different kinds of action.
        let tool = action_at(CENTER, at(0.0, -TOOL_RADIUS));
        let color = action_at(CENTER, at(0.0, -COLOR_RADIUS));
        assert!(matches!(tool, Some(RadialAction::Tool(_))));
        assert!(matches!(color, Some(RadialAction::Color(_))));
    }

    #[test]
    fn every_direction_in_the_tool_ring_resolves() {
        // Sweep the whole circle; no angle may fall through a crack.
        for step in 0..360 {
            let angle = (step as f32).to_radians();
            let probe = at(angle.sin() * TOOL_RADIUS, -angle.cos() * TOOL_RADIUS);
            assert!(
                matches!(action_at(CENTER, probe), Some(RadialAction::Tool(_))),
                "angle {step}° left no tool selected"
            );
        }
    }

    #[test]
    fn slot_positions_ring_the_centre_at_the_right_radius() {
        for index in 0..TOOLS.len() {
            let position = slot_position(CENTER, index, TOOLS.len(), TOOL_RADIUS);
            assert!(
                (position.dist(CENTER) - TOOL_RADIUS).abs() < 0.01,
                "slot {index} should sit on the ring"
            );
        }
        // Slot zero is straight up.
        let top = slot_position(CENTER, 0, TOOLS.len(), TOOL_RADIUS);
        assert!((top.x - CENTER.x).abs() < 0.01);
        assert!(top.y < CENTER.y);
    }

    #[test]
    fn a_highlighted_menu_reports_what_it_would_commit() {
        let mut menu = RadialMenu::new(CENTER);
        assert_eq!(menu.highlighted(), None, "no travel yet, so nothing chosen");

        menu.cursor = at(0.0, -TOOL_RADIUS);
        assert_eq!(menu.highlighted(), Some(RadialAction::Tool(Tool::Pen)));
    }
}
