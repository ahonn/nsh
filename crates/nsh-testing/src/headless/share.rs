//! The share sheet of a headless app ([`Headless::on_share`]): the
//! platform has one, every request is recorded ([`Headless::shared`]), and
//! the test says how the user closes each sheet ([`ShareAnswer`]).

use nsh_core::{
    Command, EventKind, EventPayload, MountItem, PayloadValue, ROOT_ID, share::ShareItem,
};
use nsh_engine::EngineControl;

use super::Headless;

/// How the user closes a share sheet ([`Headless::on_share`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShareAnswer {
    /// They shared, through the activity named (as the platform names it:
    /// iOS's activity type, such as `com.apple.UIKit.activity.Mail`;
    /// `None`: no activity in particular).
    Completed(Option<String>),
    /// They closed the sheet without sharing.
    Cancelled,
}

/// Answers a share sheet: how the user closes it, for its items.
type ShareHandler = Box<dyn FnMut(&[ShareItem]) -> ShareAnswer + Send>;

/// The share sheets the app showed.
#[derive(Default)]
pub(super) struct Shares {
    /// How the user closes them; `None`: closed without sharing.
    handler: Option<ShareHandler>,
    /// Those not answered yet: their requests and items.
    pending: Vec<(u32, Vec<ShareItem>)>,
    /// The items of every one, in order.
    shared: Vec<Vec<ShareItem>>,
}

impl Shares {
    /// Records the share sheets `items` show.
    pub(super) fn record(&mut self, items: &[MountItem]) {
        for item in items {
            if let MountItem::Command {
                id: ROOT_ID,
                command: Command::Share { id, items, .. },
            } = item
            {
                self.pending.push((*id, items.clone()));
                self.shared.push(items.clone());
            }
        }
    }

    /// Closes the sheets not answered yet, as the user does.
    pub(super) fn answer(&mut self, control: &EngineControl) {
        for (id, items) in std::mem::take(&mut self.pending) {
            let answer = match &mut self.handler {
                Some(handler) => handler(&items),
                None => ShareAnswer::Cancelled,
            };
            let mut fields = vec![("id", PayloadValue::Number(f64::from(id)))];
            match answer {
                ShareAnswer::Completed(activity) => {
                    fields.push(("completed", PayloadValue::Bool(true)));
                    if let Some(activity) = activity {
                        fields.push(("activity", PayloadValue::Str(activity)));
                    }
                }
                ShareAnswer::Cancelled => fields.push(("completed", PayloadValue::Bool(false))),
            }
            control.dispatch_event(ROOT_ID, EventKind::Share, EventPayload::Map(fields));
        }
    }
}

impl Headless {
    /// The platform has a share sheet
    /// ([`nsh_core::platform::Environment::share`]), and the user closes
    /// each one the app shows (`share`, `share_from`) as `handler` says for
    /// its items. Every request is in [`Headless::shared`].
    pub fn on_share(
        &mut self,
        handler: impl FnMut(&[ShareItem]) -> ShareAnswer + Send + 'static,
    ) -> Result<(), String> {
        self.shares.handler = Some(Box::new(handler));
        self.set_environment("onShare", |env| env.share = true)
    }

    /// The items of every share sheet the app showed so far, in order.
    /// None before [`Headless::on_share`]: without a share sheet, the core
    /// answers the app's requests itself.
    pub fn shared(&self) -> &[Vec<ShareItem>] {
        &self.shares.shared
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    use nsh_core::{element::Root, platform::Os, prelude::*};
    use nsh_engine::EngineConfig;

    use super::*;

    /// The export the app shares: a file the test writes.
    fn export() -> PathBuf {
        std::env::temp_dir().join(format!("nsh-headless-share-{}.csv", std::process::id()))
    }

    /// Shares a key with a note, and an export from a button it points at;
    /// shows how each share ended.
    #[component]
    fn Sharing() -> impl IntoElement {
        let outcome = signal(String::from("none"));
        let button = element_ref();
        let show = move |answer: Result<ShareResult, String>| {
            outcome.set(match answer {
                Ok(result) => format!(
                    "completed {} via {}",
                    result.completed,
                    result.activity.as_deref().unwrap_or("nothing")
                ),
                Err(error) => format!("failed: {error}"),
            });
        };
        view()
            .child(
                view()
                    .on_press(move || {
                        let items = [
                            ShareItem::text("Thanks for buying. Your key: ABCD-1234"),
                            ShareItem::url("https://example.com/activate"),
                        ];
                        spawn(async move { show(share(items).await) });
                    })
                    .child(text("Share key")),
            )
            .child(
                view()
                    .element_ref(&button)
                    .on_press(move || {
                        let items = [ShareItem::file(export())];
                        spawn(async move { show(share_from(items, &button).await) });
                    })
                    .child(text("Export")),
            )
            .child(
                view()
                    .on_press(move || {
                        spawn(async move { show(share([]).await) });
                    })
                    .child(text("Share nothing")),
            )
            .child(
                view()
                    .on_press(move || {
                        let missing = std::env::temp_dir().join("nsh-headless-share-missing.csv");
                        spawn(async move { show(share([ShareItem::file(missing)]).await) });
                    })
                    .child(text("Share missing")),
            )
            .child(text(move || format!("outcome: {}", outcome.get())))
    }

    fn start(os: Os) -> Headless {
        let mut config = EngineConfig::new(Root::of::<Sharing>());
        config.environment.os = os;
        let mut app = Headless::start(config).unwrap();
        app.first_render().unwrap();
        app
    }

    #[test]
    fn without_a_share_sheet_a_share_ends_at_once_unshared() {
        for os in [Os::Ios, Os::Web] {
            let mut app = start(os);
            app.press("Share key").unwrap();
            app.wait_for("text:outcome: completed false via nothing")
                .unwrap();
            assert!(app.shared().is_empty(), "{os:?}");
        }
    }

    #[test]
    fn the_test_closes_each_share_sheet_and_sees_what_it_shared() {
        std::fs::write(export(), "key,product\nABCD-1234,Keymana\n").unwrap();
        for os in [Os::Ios, Os::Web] {
            let mut app = start(os);
            let asked = Arc::new(Mutex::new(Vec::new()));
            let seen = asked.clone();
            app.on_share(move |items| {
                seen.lock().unwrap().push(items.len());
                match items {
                    [ShareItem::File(_)] => ShareAnswer::Cancelled,
                    _ => ShareAnswer::Completed(Some("com.apple.UIKit.activity.Mail".into())),
                }
            })
            .unwrap();
            // Where each sheet's popover points: the surface, then the
            // export's button.
            let anchors = Arc::new(Mutex::new(Vec::new()));
            let pointed = anchors.clone();
            app.observe(move |_, batch| {
                for item in &batch.items {
                    if let MountItem::Command {
                        command: Command::Share { anchor, .. },
                        ..
                    } = item
                    {
                        pointed.lock().unwrap().push(*anchor);
                    }
                }
            });

            app.press("Share key").unwrap();
            app.wait_for("text:outcome: completed true via com.apple.UIKit.activity.Mail")
                .unwrap();
            app.press("Export").unwrap();
            app.wait_for("text:outcome: completed false via nothing")
                .unwrap();
            assert_eq!(
                app.shared(),
                [
                    vec![
                        ShareItem::text("Thanks for buying. Your key: ABCD-1234"),
                        ShareItem::url("https://example.com/activate"),
                    ],
                    vec![ShareItem::file(export())],
                ],
                "{os:?}"
            );
            assert_eq!(*asked.lock().unwrap(), [2, 1], "{os:?}");
            let anchors = anchors.lock().unwrap();
            let button = app.screen().press_target("Export").unwrap();
            assert_eq!(*anchors, [None, button], "{os:?}");
        }
        let _ = std::fs::remove_file(export());
    }

    #[test]
    fn items_that_cannot_be_shared_fail_without_a_sheet() {
        let mut app = start(Os::Ios);
        app.on_share(|_| ShareAnswer::Completed(None)).unwrap();
        app.press("Share nothing").unwrap();
        app.wait_for("text:outcome: failed: share: nothing to share")
            .unwrap();
        app.press("Share missing").unwrap();
        app.wait_until("a missing file failing", super::super::WAIT, |screen| {
            screen.texts().iter().any(|t| {
                t.starts_with("outcome: failed: share: ")
                    && t.contains("nsh-headless-share-missing.csv")
            })
        })
        .unwrap();
        assert!(app.shared().is_empty());
    }
}
