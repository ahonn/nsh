//! Swipe actions of a view (`Interactive::leading_swipe_actions`,
//! `Interactive::trailing_swipe_actions`): buttons a sideways swipe
//! reveals at its edges, as Mail's rows do ([`crate::swipe`]).
//!
//! Each binding of an edge's actions gives their handlers ids in
//! [`super::menus::ViewMenus`] under a generation of its own, which a
//! cleanup drops when the actions are bound again (a row of a list that
//! shows another item) or the view goes. The platform reports a pressed
//! button's id as the root's [`crate::EventKind::Menu`], and the core runs
//! its handler, as it does a header bar button's.

use std::{fmt, rc::Rc, sync::Arc};

use super::{
    IntoReactive, builder::ElNode, cleanup, menus::MenuHandler, reactive::Owner, view_menus,
};
use crate::{
    Hsla, Prop,
    swipe::{SwipeButton, SwipeEdge},
};

/// A button a swipe reveals at a view's edge
/// (`Interactive::leading_swipe_actions`,
/// `Interactive::trailing_swipe_actions`): its label, an optional SF
/// Symbol above it, its color and what pressing it does.
///
/// Reversible actions go on the leading edge, irreversible ones on the
/// trailing edge, marked [`Self::destructive`]; keep to two per edge. A
/// swipe only reveals the buttons: a full swipe never runs one.
///
/// ```ignore
/// view()
///     .leading_swipe_actions(move || {
///         vec![SwipeAction::new("Renew").symbol("arrow.clockwise").on_press(renew)]
///     })
///     .trailing_swipe_actions(move || {
///         vec![SwipeAction::new("Revoke").symbol("xmark.circle").destructive(true).on_press(revoke)]
///     })
/// ```
#[derive(Clone)]
pub struct SwipeAction {
    label: Arc<str>,
    symbol: Option<Arc<str>>,
    destructive: bool,
    tint: Option<Hsla>,
    on_press: Option<MenuHandler>,
}

impl SwipeAction {
    /// A button that says `label`, also its name for screen readers, which
    /// offer it as a custom action of the view.
    pub fn new(label: impl Into<Arc<str>>) -> Self {
        Self {
            label: label.into(),
            symbol: None,
            destructive: false,
            tint: None,
            on_press: None,
        }
    }

    /// An SF Symbol shown above the label (iOS).
    pub fn symbol(mut self, name: impl Into<Arc<str>>) -> Self {
        self.symbol = Some(name.into());
        self
    }

    /// An action that cannot be undone: the button is the system's red.
    pub fn destructive(mut self, destructive: bool) -> Self {
        self.destructive = destructive;
        self
    }

    /// The button's color when it is not destructive; the system's by
    /// default (blue at the leading edge, gray at the trailing edge).
    pub fn tint(mut self, color: impl Into<Hsla>) -> Self {
        self.tint = Some(color.into());
        self
    }

    /// Runs `f` when the button is pressed (or a screen reader runs the
    /// action); the view closes first.
    pub fn on_press(mut self, f: impl Fn() + 'static) -> Self {
        self.on_press = Some(Rc::new(f));
        self
    }
}

impl fmt::Debug for SwipeAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SwipeAction")
            .field("label", &self.label)
            .field("symbol", &self.symbol)
            .field("destructive", &self.destructive)
            .field("tint", &self.tint)
            .finish_non_exhaustive()
    }
}

crate::static_reactive!(Vec<SwipeAction>: Vec<SwipeAction>);

/// Binds the actions of `edge` of `node`: now when they are static, else
/// in the node's binding, each run registering its handlers.
pub(crate) fn bind(
    node: &mut ElNode,
    edge: SwipeEdge,
    actions: impl IntoReactive<Vec<SwipeAction>>,
) {
    super::host::bind_prop(node, actions, move |actions| {
        let buttons = register(actions);
        match edge {
            SwipeEdge::Leading => Prop::LeadingSwipeActions(buttons),
            SwipeEdge::Trailing => Prop::TrailingSwipeActions(buttons),
        }
    });
}

/// Gives `actions`' handlers ids, keeps them under a generation that a
/// cleanup of the current owner drops, and returns the buttons the
/// platform shows.
fn register(actions: Vec<SwipeAction>) -> Vec<SwipeButton> {
    if actions.is_empty() {
        return Vec::new();
    }
    let menus = view_menus();
    let owner = Owner::current();
    let mut handlers = Vec::new();
    let buttons = actions
        .into_iter()
        .map(|action| {
            let id = match action.on_press {
                Some(f) => {
                    let id = menus.borrow_mut().next_id();
                    handlers.push((id, Rc::new(move || owner.run(&*f)) as MenuHandler));
                    id
                }
                None => 0,
            };
            SwipeButton::from_parts(
                action.label,
                action.symbol,
                action.destructive,
                action.tint.map(Into::into),
                id,
            )
        })
        .collect();
    let generation = menus.borrow_mut().push_swipe(handlers);
    let weak = Rc::downgrade(&menus);
    cleanup(move || {
        if let Some(menus) = weak.upgrade() {
            menus.borrow_mut().take_swipe(generation);
        }
    });
    buttons
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::element::{Runtime, create_root, signal};

    fn buttons(node: &ElNode, edge: SwipeEdge) -> Vec<SwipeButton> {
        node.resolved_attrs()
            .props
            .into_iter()
            .find_map(|p| match (p, edge) {
                (Prop::LeadingSwipeActions(b), SwipeEdge::Leading)
                | (Prop::TrailingSwipeActions(b), SwipeEdge::Trailing) => Some(b),
                _ => None,
            })
            .unwrap_or_default()
    }

    fn run(id: u32) -> bool {
        match view_menus().borrow().handler(id) {
            Some(handler) => {
                handler();
                true
            }
            None => false,
        }
    }

    #[test]
    fn static_actions_get_ids_and_their_handlers_run() {
        let runtime = Runtime::default();
        runtime.set_services(Rc::new(crate::element::Executor::default()));
        let _entered = runtime.enter();
        let renewed = Rc::new(Cell::new(0));
        let r = renewed.clone();
        let mut node = ElNode::new(crate::Tag::View);
        bind(
            &mut node,
            SwipeEdge::Leading,
            vec![
                SwipeAction::new("Renew")
                    .symbol("arrow.clockwise")
                    .on_press(move || r.set(r.get() + 1)),
                SwipeAction::new("Later").tint(crate::color::rgb(0xff9500)),
            ],
        );
        let leading = buttons(&node, SwipeEdge::Leading);
        assert_eq!(leading.len(), 2);
        assert_eq!(&*leading[0].label, "Renew");
        assert_eq!(leading[0].symbol.as_deref(), Some("arrow.clockwise"));
        assert!(leading[0].id >= super::super::menus::FIRST_ID);
        assert_eq!(leading[1].id, 0, "no handler, no id");
        assert!(leading[1].tint.is_some());
        assert!(run(leading[0].id));
        assert_eq!(renewed.get(), 1);
    }

    #[test]
    fn a_bindings_handlers_go_with_its_owner() {
        let runtime = Runtime::default();
        runtime.set_services(Rc::new(crate::element::Executor::default()));
        let _entered = runtime.enter();
        let item = signal(0_u32);
        let revoked = Rc::new(Cell::new(Vec::new()));
        let r = revoked.clone();
        let mut node = ElNode::new(crate::Tag::View);
        bind(&mut node, SwipeEdge::Trailing, move || {
            let at = item.get();
            let r = r.clone();
            vec![
                SwipeAction::new("Revoke")
                    .destructive(true)
                    .on_press(move || r.set([r.take(), vec![at]].concat())),
            ]
        });
        // Each run registers under the owner it runs in (the core's
        // binding effect); a run reads the item it is bound to.
        let (first_owner, first) = create_root(|| buttons(&node, SwipeEdge::Trailing));
        item.set(1);
        let (_second_owner, second) = create_root(|| buttons(&node, SwipeEdge::Trailing));
        assert!(first[0].destructive);
        assert_ne!(first[0].id, second[0].id);
        assert!(run(first[0].id));
        assert!(run(second[0].id));
        assert_eq!(revoked.take(), vec![0, 1]);
        first_owner.dispose();
        assert!(!run(first[0].id), "the first run's handler is gone");
        assert!(run(second[0].id));
        assert_eq!(revoked.take(), vec![1]);
    }
}
