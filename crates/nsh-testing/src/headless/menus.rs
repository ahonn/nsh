//! The context menu steps: `rightClick(<selector>)`, the mouse's secondary
//! button over a view, and choosing from (or closing) the menu the app
//! then shows with `Menu.popup`, as a user does on a desktop, or the
//! context menu of a view (`Interactive::context_menu`) a long press opened
//! (`longPress(<selector>)`), as on a touch screen.

use nsh_core::{
    ContextMenuEvent, EventKind, EventPayload, PayloadValue, PointerType, ROOT_ID, geometry::point,
};

use super::Headless;
use crate::{menu, selector::Selector};

impl Headless {
    /// The step `rightClick(<selector>)`: the mouse's secondary button over
    /// the middle of the view `selector` names, on a desktop (`open`'s
    /// `platform`): the view, or the nearest around it listening, hears
    /// `onContextMenu`; its handler shows a menu (`Menu.popup`) or its own
    /// UI. A phone has no mouse: there it fails.
    pub fn right_click(&mut self, selector: &str) -> Result<(), String> {
        let parsed = Selector::parse(selector)?;
        self.step(format!("rightClick({selector})"));
        if !self.environment.desktop() {
            return Err(
                "a phone has no mouse to right-click with: open the app as a desktop (`platform`)"
                    .into(),
            );
        }
        let pick = self.screen.view(&parsed);
        let target = self.picked(pick, selector, "view")?;
        let at = self
            .screen
            .bounds_in_root(target)
            .ok_or_else(|| format!("`{selector}` is not on screen"))?
            .center();
        self.activate_window_of(target);
        if let Some(id) = self.screen.context_menu_target(target) {
            let origin = self
                .screen
                .bounds_in_root(id)
                .map_or(Default::default(), |b| b.origin);
            let event = ContextMenuEvent::new(
                point(at.x - origin.x, at.y - origin.y),
                at,
                Default::default(),
                Some(PointerType::Mouse),
            );
            self.send(id, EventKind::ContextMenu, EventPayload::ContextMenu(event));
        }
        self.done()
    }

    /// The actions of the context menu the app shows (`Menu.popup`, or a
    /// view's a long press opened), `None` when none is open.
    pub fn menu(&self) -> Option<Vec<menu::Entry>> {
        self.screen.menu().map(menu::entries)
    }

    /// The step `chooseMenuItem(<label>)`: chooses the open context menu's
    /// action labelled `label` (a submenu's: `"Share > Mail"`), as a click
    /// on it does: the app hears `onMenu` with its id, then the menu closes.
    pub fn choose_menu_item(&mut self, label: &str) -> Result<(), String> {
        self.step(format!("chooseMenuItem({label})"));
        let items = self.screen.menu().ok_or("no context menu is open")?;
        let action = menu::find(items, label)?;
        let id = action.id;
        if id != 0 {
            self.menu_event(vec![("id", PayloadValue::Number(f64::from(id)))]);
        }
        self.close_open_menu();
        self.done()
    }

    /// The step `closeMenu()`: the open context menu closes with nothing
    /// chosen, as Escape or a click elsewhere closes it.
    pub fn close_menu(&mut self) -> Result<(), String> {
        self.step("closeMenu");
        if self.screen.menu().is_none() {
            return Err("no context menu is open".into());
        }
        self.close_open_menu();
        self.done()
    }

    fn close_open_menu(&mut self) {
        let popup = self.screen.menu_is_popup();
        self.screen.menu_closed();
        // The app's `Menu.popup` promise settles; a view's context menu
        // closes natively, with nothing to settle.
        if popup {
            self.menu_event(vec![
                ("id", PayloadValue::Number(0.0)),
                ("closed", PayloadValue::Bool(true)),
            ]);
        }
    }

    fn menu_event(&self, fields: Vec<(&'static str, PayloadValue)>) {
        self.engine
            .control()
            .dispatch_event(ROOT_ID, EventKind::Menu, EventPayload::Map(fields));
    }

    /// The step `dropFiles(<selector>, <uris>)`: local files (`file:`
    /// URLs) dragged from a file manager and dropped on the middle of the
    /// view `selector` names; it, or the nearest view around it listening,
    /// hears `onFileDrop`. Desktops only, as the platforms report it.
    pub fn drop_files(&mut self, selector: &str, uris: Vec<String>) -> Result<(), String> {
        let parsed = Selector::parse(selector)?;
        self.step(format!("dropFiles({selector})"));
        if !self.environment.desktop() {
            return Err("files are dropped on a desktop: open the app as one (`platform`)".into());
        }
        let pick = self.screen.view(&parsed);
        let target = self.picked(pick, selector, "view")?;
        let Some(id) = self
            .screen
            .chain(target)
            .into_iter()
            .find(|&id| self.screen.hears(id, EventKind::FileDrop))
        else {
            return Err(format!("no view at `{selector}` listens to `onFileDrop`"));
        };
        // Let go at the middle of what the selector names, as Linux
        // reports where.
        let at = self
            .screen
            .match_inside(&parsed, target)
            .and_then(|inner| self.screen.bounds_in_root(inner))
            .or_else(|| self.screen.bounds_in_root(target));
        let payload = match at {
            Some(b) => EventPayload::FileDropAt {
                uris,
                page_position: b.center(),
            },
            None => EventPayload::FileDrop(uris),
        };
        self.send(id, EventKind::FileDrop, payload);
        self.done()
    }
}

#[cfg(test)]
mod tests {
    use nsh_core::{
        element::{Root, desktop::MenuItem},
        platform::Os,
        prelude::*,
    };
    use nsh_engine::EngineConfig;

    use super::*;

    /// A row that opens on a press and has a context menu with an inline
    /// section, and a view that hears long presses alone.
    #[component]
    fn Rows() -> impl IntoElement {
        let log = signal(String::from("none"));
        view()
            .child(text(move || format!("log {}", log.get())))
            .child(
                view()
                    .label("Key row")
                    .on_press(move || log.set("opened".into()))
                    .on_long_press(move || log.set("long".into()))
                    .context_menu(move || {
                        vec![
                            MenuItem::new("Copy key")
                                .symbol("doc.on.doc")
                                .on_select(move || log.set("copied".into())),
                            MenuItem::submenu(
                                "",
                                [MenuItem::new("Revoke")
                                    .destructive(true)
                                    .on_select(move || log.set("revoked".into()))],
                            )
                            .inline(true),
                        ]
                    })
                    .child(text("Key")),
            )
            .child(
                view()
                    .label("Plain")
                    .on_long_press(move || log.set("plain long".into()))
                    .child(text("Plain text")),
            )
    }

    fn start(os: Os) -> Headless {
        let mut config = EngineConfig::new(Root::of::<Rows>());
        config.environment.os = os;
        let mut app = Headless::start(config).unwrap();
        app.first_render().unwrap();
        app
    }

    fn labels(app: &Headless) -> Option<Vec<String>> {
        app.menu()
            .map(|entries| entries.into_iter().map(|e| e.label).collect())
    }

    #[test]
    fn a_long_press_opens_a_views_context_menu_on_a_phone_and_the_web() {
        for os in [Os::Ios, Os::Web] {
            let mut app = start(os);
            app.long_press("Key row").unwrap();
            // The menu takes the hold: no press, no long press.
            assert_eq!(
                labels(&app),
                Some(vec!["Copy key".to_owned(), "Revoke".to_owned()]),
                "{os:?}"
            );
            assert!(app.shows("log none"), "{os:?}");
            app.choose_menu_item("Revoke").unwrap();
            app.wait_for("log revoked").unwrap();
            assert_eq!(app.menu(), None);

            // Inside the row, the row's menu opens.
            app.long_press("Key").unwrap();
            assert_eq!(labels(&app).map(|l| l.len()), Some(2), "{os:?}");
            app.close_menu().unwrap();
            assert_eq!(app.menu(), None);
            assert!(app.shows("log revoked"), "{os:?}");
            assert!(app.choose_menu_item("Copy key").is_err());

            // Presses and long presses elsewhere are as they were.
            app.press("Key row").unwrap();
            app.wait_for("log opened").unwrap();
            app.long_press("Plain").unwrap();
            app.wait_for("log plain long").unwrap();
            assert_eq!(app.menu(), None);
        }
    }
}
