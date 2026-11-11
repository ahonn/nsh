//! Swipe actions: buttons a view reveals at its leading or trailing edge
//! when it is swiped sideways, as Mail's rows do (Rust apps:
//! [`crate::element::SwipeAction`], `Interactive::leading_swipe_actions` /
//! `trailing_swipe_actions`). This module holds what the platforms see
//! ([`SwipeActions`] in [`crate::ViewProps::swipe_actions`]) and the
//! decisions of the gesture that do not depend on a toolkit, so every
//! backend that shows them moves the same way.
//!
//! A press of a button (or a screen reader's custom action of the same
//! name) is reported as the root's [`crate::EventKind::Menu`] with the
//! button's [`SwipeButton::id`], as header bar buttons are; the core runs
//! the action's handler. A swipe only reveals the buttons: a full swipe
//! never runs one.
//!
//! The gesture, in the view's own direction (`rtl` mirrors it):
//!
//! - It starts only for a sideways move ([`may_start`]) toward an edge
//!   that has actions, or either way while the view is open.
//! - The view follows the finger; past the revealed buttons' total width
//!   it resists ([`rubber_band`]).
//! - On release it settles open when it is past half of that width
//!   ([`settles_open`]), else closed.
//! - The buttons share what is revealed in proportion to their widths,
//!   the first at the outer edge ([`button_frames`]).

use std::sync::Arc;

use crate::{
    Color,
    geometry::{Point, Points, px},
};

/// The narrowest a button is, as UIKit's swipe action buttons.
pub const MIN_BUTTON_WIDTH: Points = px(74.0);

/// How far the view moves past its buttons for a move of the same length
/// past them, at the start (it resists more the further it goes).
const RUBBER_BAND: f32 = 0.55;

/// One button of a view's swipe actions, as the platform shows it.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct SwipeButton {
    /// What it says, also its name for screen readers.
    pub label: Arc<str>,
    /// An SF Symbol shown above the label (iOS).
    pub symbol: Option<Arc<str>>,
    /// An action that cannot be undone: the system's red.
    pub destructive: bool,
    /// The background of a button that is not destructive; the platform's
    /// default when `None`.
    pub tint: Option<Color>,
    /// Reported as the root's [`crate::EventKind::Menu`] when pressed (0:
    /// no handler, the press only closes the view).
    pub id: u32,
}

impl SwipeButton {
    /// A button from its fields. `#[non_exhaustive]` keeps a struct
    /// literal off other crates; Rust apps build
    /// [`crate::element::SwipeAction`]s instead.
    #[doc(hidden)]
    pub fn from_parts(
        label: Arc<str>,
        symbol: Option<Arc<str>>,
        destructive: bool,
        tint: Option<Color>,
        id: u32,
    ) -> Self {
        Self {
            label,
            symbol,
            destructive,
            tint,
            id,
        }
    }

    /// Whether `other` looks the same (only the id may differ): a view
    /// open on these buttons can stay open.
    pub fn looks_like(&self, other: &SwipeButton) -> bool {
        self.label == other.label
            && self.symbol == other.symbol
            && self.destructive == other.destructive
            && self.tint == other.tint
    }
}

/// An edge of a view whose swipe actions a swipe reveals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SwipeEdge {
    /// Where lines start (the left left to right): revealed by a swipe
    /// toward the trailing edge.
    Leading,
    /// Where lines end: revealed by a swipe toward the leading edge.
    Trailing,
}

impl SwipeEdge {
    /// The edge a view moved by `offset` (positive to the right) reveals:
    /// the left one, leading left to right, when it moved right.
    pub fn revealed_by(offset: Points, rtl: bool) -> Option<SwipeEdge> {
        let right = if offset.0 > 0.0 {
            true
        } else if offset.0 < 0.0 {
            false
        } else {
            return None;
        };
        Some(if right != rtl {
            SwipeEdge::Leading
        } else {
            SwipeEdge::Trailing
        })
    }

    /// Whether the view moves right to reveal this edge.
    pub fn moves_right(self, rtl: bool) -> bool {
        (self == SwipeEdge::Leading) != rtl
    }
}

/// A view's swipe actions, as the platform shows them
/// ([`crate::ViewProps::swipe_actions`]); empty on both edges, the view
/// has no swipe gesture.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct SwipeActions {
    /// Revealed at the leading edge, the first at the outer edge.
    pub leading: Vec<SwipeButton>,
    /// Revealed at the trailing edge, the first at the outer edge.
    pub trailing: Vec<SwipeButton>,
}

impl SwipeActions {
    /// Whether neither edge has an action.
    pub fn is_empty(&self) -> bool {
        self.leading.is_empty() && self.trailing.is_empty()
    }

    /// The buttons of `edge`.
    pub fn edge(&self, edge: SwipeEdge) -> &[SwipeButton] {
        match edge {
            SwipeEdge::Leading => &self.leading,
            SwipeEdge::Trailing => &self.trailing,
        }
    }

    /// Every button, the leading edge's first.
    pub fn buttons(&self) -> impl Iterator<Item = (SwipeEdge, &SwipeButton)> {
        let leading = self.leading.iter().map(|b| (SwipeEdge::Leading, b));
        let trailing = self.trailing.iter().map(|b| (SwipeEdge::Trailing, b));
        leading.chain(trailing)
    }
}

/// Whether a pan whose finger moves by `motion` (a translation or a
/// velocity: only its direction counts) may start swiping a view: a
/// sideways move (more across than along), toward an edge with actions
/// while the view is closed (`open` is `None`), or either way while it is
/// open, to close it or drag it further.
pub fn may_start(
    motion: Point<Points>,
    actions: &SwipeActions,
    open: Option<SwipeEdge>,
    rtl: bool,
) -> bool {
    if motion.x.0.abs() <= motion.y.0.abs() {
        return false;
    }
    if open.is_some() {
        return true;
    }
    SwipeEdge::revealed_by(motion.x, rtl).is_some_and(|edge| !actions.edge(edge).is_empty())
}

/// Where the view is for a finger at `offset` from where it rests closed,
/// with `width` revealed at most before it resists: as far as the finger
/// up to `width` either way, then less and less (UIKit's rubber band, its
/// constant scaled by `dimension`, the view's width), and nowhere toward
/// an edge without actions (`width` zero).
pub fn rubber_band(offset: Points, width: Points, dimension: Points) -> Points {
    let distance = offset.0.abs();
    let limit = width.0.max(0.0);
    if limit == 0.0 {
        return Points::ZERO;
    }
    if distance <= limit {
        return offset;
    }
    let excess = distance - limit;
    let d = dimension.0.max(1.0);
    let resisted = excess * d * RUBBER_BAND / (d + RUBBER_BAND * excess);
    Points((limit + resisted).copysign(offset.0))
}

/// Whether a view released at `projected` (where it is, plus where its
/// speed carries it shortly: a flick counts) settles open on buttons
/// `width` wide: past half of them.
pub fn settles_open(projected: Points, width: Points) -> bool {
    width.0 > 0.0 && projected.0.abs() > width.0 / 2.0
}

/// The frames of buttons whose own widths are `widths` when `revealed`
/// of them shows: each its share of what is revealed, from the first at
/// the outer edge (the left one when `outer_left`) inward. Each frame is
/// its origin across the revealed area (from its left) and its width.
pub fn button_frames(
    widths: &[Points],
    revealed: Points,
    outer_left: bool,
) -> Vec<(Points, Points)> {
    let total: f32 = widths.iter().map(|w| w.0).sum();
    if total <= 0.0 {
        return widths
            .iter()
            .map(|_| (Points::ZERO, Points::ZERO))
            .collect();
    }
    let scale = revealed.0.max(0.0) / total;
    let mut from_outer = 0.0;
    widths
        .iter()
        .map(|w| {
            let width = w.0 * scale;
            let start = if outer_left {
                from_outer
            } else {
                revealed.0 - from_outer - width
            };
            from_outer += width;
            (Points(start), Points(width))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::point;

    fn button(label: &str) -> SwipeButton {
        SwipeButton {
            label: label.into(),
            ..Default::default()
        }
    }

    fn actions(leading: &[&str], trailing: &[&str]) -> SwipeActions {
        SwipeActions {
            leading: leading.iter().map(|l| button(l)).collect(),
            trailing: trailing.iter().map(|l| button(l)).collect(),
        }
    }

    #[test]
    fn a_swipe_right_reveals_the_leading_edge_left_to_right_only() {
        assert_eq!(
            SwipeEdge::revealed_by(px(10.0), false),
            Some(SwipeEdge::Leading)
        );
        assert_eq!(
            SwipeEdge::revealed_by(px(-10.0), false),
            Some(SwipeEdge::Trailing)
        );
        assert_eq!(
            SwipeEdge::revealed_by(px(10.0), true),
            Some(SwipeEdge::Trailing)
        );
        assert_eq!(
            SwipeEdge::revealed_by(px(-10.0), true),
            Some(SwipeEdge::Leading)
        );
        assert_eq!(SwipeEdge::revealed_by(Points::ZERO, false), None);
        assert!(SwipeEdge::Leading.moves_right(false));
        assert!(!SwipeEdge::Leading.moves_right(true));
        assert!(SwipeEdge::Trailing.moves_right(true));
    }

    #[test]
    fn only_a_sideways_move_toward_an_edge_with_actions_starts() {
        let trailing_only = actions(&[], &["Revoke"]);
        let left = point(px(-20.0), px(5.0));
        let right = point(px(20.0), px(5.0));
        assert!(may_start(left, &trailing_only, None, false));
        assert!(
            !may_start(right, &trailing_only, None, false),
            "no leading actions: a swipe right is the page's (going back)"
        );
        assert!(
            !may_start(point(px(5.0), px(20.0)), &trailing_only, None, false),
            "a vertical move scrolls"
        );
        assert!(
            !may_start(point(px(-10.0), px(10.0)), &trailing_only, None, false),
            "a diagonal move scrolls"
        );
        assert!(may_start(right, &trailing_only, None, true), "mirrored");
        assert!(
            may_start(right, &trailing_only, Some(SwipeEdge::Trailing), false),
            "an open view closes either way"
        );
    }

    #[test]
    fn the_view_follows_the_finger_up_to_its_buttons_then_resists() {
        let (width, row) = (px(148.0), px(390.0));
        assert_eq!(rubber_band(px(-60.0), width, row), px(-60.0));
        assert_eq!(rubber_band(px(-148.0), width, row), px(-148.0));
        let past = rubber_band(px(-248.0), width, row);
        assert!(past.0 < -148.0 && past.0 > -248.0, "{past:?}");
        let further = rubber_band(px(-348.0), width, row);
        assert!(
            further.0 - past.0 > -100.0 * RUBBER_BAND,
            "resists more the further it goes"
        );
        assert_eq!(rubber_band(px(40.0), Points::ZERO, row), Points::ZERO);
    }

    #[test]
    fn a_release_past_half_the_buttons_keeps_the_view_open() {
        let width = px(148.0);
        assert!(settles_open(px(-80.0), width));
        assert!(settles_open(px(80.0), width));
        assert!(!settles_open(px(-60.0), width));
        assert!(!settles_open(px(-200.0), Points::ZERO));
    }

    #[test]
    fn buttons_share_what_is_revealed_from_the_outer_edge() {
        let widths = [px(74.0), px(100.0)];
        // Fully revealed at the left: the first at the left edge.
        assert_eq!(
            button_frames(&widths, px(174.0), true),
            vec![(px(0.0), px(74.0)), (px(74.0), px(100.0))]
        );
        // At the right: the first at the right edge.
        assert_eq!(
            button_frames(&widths, px(174.0), false),
            vec![(px(100.0), px(74.0)), (px(0.0), px(100.0))]
        );
        // Half revealed: half of each.
        assert_eq!(
            button_frames(&widths, px(87.0), true),
            vec![(px(0.0), px(37.0)), (px(37.0), px(50.0))]
        );
        assert!(button_frames(&[], px(50.0), true).is_empty());
    }

    #[test]
    fn a_view_stays_open_on_buttons_that_look_the_same() {
        let a = SwipeButton {
            id: 1,
            ..button("Revoke")
        };
        let b = SwipeButton {
            id: 2,
            ..button("Revoke")
        };
        assert!(a.looks_like(&b));
        assert!(!a.looks_like(&SwipeButton {
            destructive: true,
            ..b
        }));
    }
}
