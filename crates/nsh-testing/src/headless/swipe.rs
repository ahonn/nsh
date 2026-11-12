//! The swipe action steps: what a sideways swipe reveals on a view
//! (`Interactive::leading_swipe_actions` / `trailing_swipe_actions`), and
//! pressing one of its buttons, as a user who swiped the view open does.
//! They need no platform that shows the buttons: the steps read what the
//! core mounted and report the button's id as the platforms do.

use nsh_core::{
    EventKind, EventPayload, PayloadValue, ROOT_ID,
    swipe::{SwipeActions, SwipeButton, SwipeEdge},
};

use super::Headless;
use crate::selector::Selector;

impl Headless {
    /// The swipe actions of the view `selector` names, or of its nearest
    /// ancestor that has some (a list row, named by a text in it).
    pub fn swipe_actions(&self, selector: &str) -> Result<SwipeActions, String> {
        let parsed = Selector::parse(selector)?;
        let pick = self.screen.swipeable(&parsed);
        let id = self.picked(pick, selector, "view with swipe actions")?;
        Ok(self.screen.swipe_actions(id).cloned().unwrap_or_default())
    }

    /// The step `swipeAction(<selector>, <label>)`: swipes the view
    /// `selector` names (or its nearest ancestor with swipe actions) open
    /// and presses its button labelled `label`, on either edge: the app's
    /// handler of that action runs. Fails when the view has no such
    /// action.
    pub fn swipe_action(&mut self, selector: &str, label: &str) -> Result<(), String> {
        let parsed = Selector::parse(selector)?;
        self.step(format!("swipeAction({selector}, {label})"));
        let pick = self.screen.swipeable(&parsed);
        let id = self.picked(pick, selector, "view with swipe actions")?;
        let actions = self.screen.swipe_actions(id).cloned().unwrap_or_default();
        let Some((_, button)) = actions.buttons().find(|(_, b)| &*b.label == label) else {
            return Err(format!(
                "`{selector}` has no swipe action `{label}`; it has {}",
                describe(&actions)
            ));
        };
        if button.id != 0 {
            self.engine.control().dispatch_event(
                ROOT_ID,
                EventKind::Menu,
                EventPayload::Map(vec![("id", PayloadValue::Number(f64::from(button.id)))]),
            );
        }
        self.done()
    }
}

/// The labels of `actions` by edge, for a failure.
fn describe(actions: &SwipeActions) -> String {
    let labels = |edge| {
        actions
            .edge(edge)
            .iter()
            .map(|b: &SwipeButton| format!("{:?}", &*b.label))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "leading [{}], trailing [{}]",
        labels(SwipeEdge::Leading),
        labels(SwipeEdge::Trailing)
    )
}

#[cfg(test)]
mod tests {
    use nsh_core::{
        element::Root,
        platform::Os,
        prelude::*,
        swipe::{SwipeButton, SwipeEdge},
    };
    use nsh_engine::EngineConfig;

    use super::*;

    /// A long list whose rows renew (leading) and revoke (trailing) their
    /// own item, and a log of what ran.
    #[component]
    fn Keys() -> impl IntoElement {
        let log = signal(String::from("none"));
        view()
            .flex_1()
            .child(text(move || format!("log: {}", log.get())))
            .child(view().child(text("No actions")))
            .child(
                virtual_list(100_usize, move |at| {
                    view()
                        .h(px(60.0))
                        .leading_swipe_actions(move || {
                            let i = at.index();
                            vec![
                                SwipeAction::new("Renew")
                                    .symbol("arrow.clockwise")
                                    .on_press(move || log.set(format!("renew {i}"))),
                            ]
                        })
                        .trailing_swipe_actions(move || {
                            let i = at.index();
                            vec![
                                SwipeAction::new("Revoke")
                                    .destructive(true)
                                    .on_press(move || log.set(format!("revoke {i}"))),
                            ]
                        })
                        .child(text(move || format!("Key {}", at.index())))
                })
                .test_id("keys")
                .flex_1(),
            )
    }

    fn start(os: Os) -> Headless {
        let mut config = EngineConfig::new(Root::of::<Keys>());
        config.environment.os = os;
        let mut app = Headless::start(config).unwrap();
        app.first_render().unwrap();
        app.wait_for("Key 3").unwrap();
        app
    }

    fn labels(buttons: &[SwipeButton]) -> Vec<&str> {
        buttons.iter().map(|b| &*b.label).collect()
    }

    #[test]
    fn a_rows_swipe_actions_are_listed_by_a_text_in_it() {
        let app = start(Os::Ios);
        let actions = app.swipe_actions("Key 3").unwrap();
        assert_eq!(labels(&actions.leading), ["Renew"]);
        assert_eq!(
            actions.leading[0].symbol.as_deref(),
            Some("arrow.clockwise")
        );
        assert_eq!(labels(&actions.trailing), ["Revoke"]);
        assert!(actions.trailing[0].destructive);
        assert_eq!(
            actions.buttons().map(|(edge, _)| edge).collect::<Vec<_>>(),
            [SwipeEdge::Leading, SwipeEdge::Trailing]
        );
        let error = app.swipe_actions("No actions").unwrap_err();
        assert!(
            error.starts_with("no view with swipe actions matches `No actions`"),
            "{error}"
        );
    }

    #[test]
    fn a_swipe_action_runs_its_rows_handler_on_ios_and_the_web() {
        for os in [Os::Ios, Os::Web] {
            let mut app = start(os);
            app.swipe_action("Key 3", "Revoke").unwrap();
            app.wait_for("text:log: revoke 3").unwrap();
            app.swipe_action("Key 5", "Renew").unwrap();
            app.wait_for("text:log: renew 5").unwrap();
            let error = app.swipe_action("Key 5", "Delete").unwrap_err();
            assert_eq!(
                error,
                "`Key 5` has no swipe action `Delete`; it has leading [\"Renew\"], trailing \
                 [\"Revoke\"]"
            );
        }
    }

    #[test]
    fn a_recycled_rows_actions_follow_its_new_item() {
        let mut app = start(Os::Ios);
        app.scroll("id:keys", px(3000.0)).unwrap();
        app.wait_for("Key 60").unwrap();
        assert!(
            !app.shows("Key 2"),
            "the first rows' cells show later items"
        );
        app.swipe_action("Key 60", "Revoke").unwrap();
        app.wait_for("text:log: revoke 60").unwrap();
        app.swipe_action("Key 61", "Renew").unwrap();
        app.wait_for("text:log: renew 61").unwrap();
    }
}
