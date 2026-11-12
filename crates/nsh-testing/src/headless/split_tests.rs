//! Split views in headless runs: every column's pages and their header
//! items are on screen to find and press, on iOS (whose columns UIKit
//! places, laid out side by side here) and on the web (a row of the
//! columns the display mode shows).

use nsh_base::{nav_page, nav_stack, split_view};
use nsh_core::{
    element::{Root, Screen},
    platform::Os,
    prelude::*,
};
use nsh_engine::EngineConfig;

use super::Headless;

const MAILBOXES: [&str; 2] = ["Inbox", "Archive"];

/// A page titled `title` whose header has a button `action` running `f`:
/// in the system header on iOS, in the page's own header on the web.
fn page(title: &'static str, action: &'static str, f: impl Fn() + 'static) -> Screen {
    let button = text(action).role(AccessibilityRole::Button).on_press(f);
    let page = nav_page().title(title);
    if os() == Os::Ios {
        page.header_right(button)
    } else {
        page.header(view().flex_row().child(text(title).flex_1()).child(button))
    }
}

/// A mail client: mailboxes, their messages, a message that pushes its
/// sender; a log of the header buttons pressed. "Focus" asks for the
/// secondary column alone.
#[component]
fn Mail() -> impl IntoElement {
    let mailbox = signal(0usize);
    let message = signal(String::from("Welcome"));
    let sender = signal(false);
    let log = signal(String::from("none"));
    let mode = signal(SplitDisplayMode::Automatic);
    let split = split_view()
        .primary(
            nav_stack().flex_1().child(
                page("Mailboxes", "Focus", move || {
                    mode.set(SplitDisplayMode::SecondaryOnly)
                })
                .child(view().children(MAILBOXES.iter().enumerate().map(
                    move |(i, name)| {
                        text(*name)
                            .role(AccessibilityRole::Button)
                            .on_press(move || mailbox.set(i))
                    },
                ))),
            ),
        )
        .supplementary(nav_stack().flex_1().child(
            page("Messages", "Compose", move || log.set("compose".into())).child(dynamic(
                move || {
                    let subjects = if mailbox.get() == 0 {
                        ["Launch plan", "Offsite photos"]
                    } else {
                        ["Old invoice", "Venue options"]
                    };
                    view().children(subjects.map(|subject| {
                        text(subject)
                            .role(AccessibilityRole::Button)
                            .on_press(move || {
                                sender.set(false);
                                message.set(subject.into());
                            })
                    }))
                },
            )),
        ))
        .secondary(
            nav_stack()
                .flex_1()
                .child(dynamic(move || {
                    let subject = message.get();
                    page("Message", "Flag", move || log.set("flag".into()))
                        .key(format!("message:{subject}"))
                        .child(text(format!("Reading {subject}")))
                        .child(
                            text("From Ava")
                                .role(AccessibilityRole::Button)
                                .on_press(move || sender.set(true)),
                        )
                }))
                .child(dynamic(move || {
                    sender.get().then(|| {
                        page("Ava", "Call", move || log.set("call".into()))
                            .key("sender")
                            .on_dismiss(move || sender.set(false))
                            .child(text("ava@example.com"))
                    })
                })),
        )
        .preferred_display_mode(move || mode.get())
        .flex_1();
    view()
        .flex_1()
        .child(text(move || format!("log: {}", log.get())))
        .child(split)
}

fn start(os: Os) -> Headless {
    let mut config = EngineConfig::new(Root::of::<Mail>());
    config.environment.os = os;
    config.viewport = nsh_core::Size::new(px(1180.0), px(820.0));
    if os == Os::Web {
        config.environment.idiom = nsh_core::platform::Idiom::Desktop;
    }
    let mut app = Headless::start(config).unwrap();
    app.first_render().unwrap();
    app
}

#[test]
fn every_columns_pages_and_header_items_can_be_found_and_pressed() {
    for os in [Os::Ios, Os::Web] {
        let mut app = start(os);
        for shown in ["Inbox", "Launch plan", "Reading Welcome"] {
            app.wait_for(shown)
                .unwrap_or_else(|e| panic!("{os:?}: {e}"));
        }
        // The list column replaces the detail column's root.
        app.press("Offsite photos").unwrap();
        app.wait_for("Reading Offsite photos").unwrap();
        assert!(!app.shows("Reading Welcome"), "{os:?}");
        // The sidebar changes the list.
        app.press("Archive").unwrap();
        app.wait_for("Venue options").unwrap();
        // The detail column pushes.
        app.press("From Ava").unwrap();
        app.wait_for("ava@example.com").unwrap();
        // Each column's header items.
        app.press("Compose").unwrap();
        app.wait_for("text:log: compose").unwrap();
        app.press("Call").unwrap();
        app.wait_for("text:log: call").unwrap();
    }
}

#[test]
fn a_row_hides_the_columns_its_display_mode_leaves_out() {
    let mut app = start(Os::Web);
    app.wait_for("Inbox").unwrap();
    app.press("Focus").unwrap();
    app.wait_for_gone("Inbox", std::time::Duration::from_secs(2))
        .unwrap();
    assert!(!app.shows("Launch plan"));
    assert!(app.shows("Reading Welcome"));
}

#[test]
fn the_size_class_follows_the_environment() {
    #[component]
    fn Classes() -> impl IntoElement {
        text(move || format!("width: {}", horizontal_size_class().name()))
    }
    for (width, reported, expected) in [
        (1180.0, SizeClass::Unspecified, "regular"),
        (390.0, SizeClass::Unspecified, "compact"),
        (1180.0, SizeClass::Compact, "compact"),
    ] {
        let mut config = EngineConfig::new(Root::of::<Classes>());
        config.environment.os = Os::Ios;
        config.viewport = nsh_core::Size::new(px(width), px(820.0));
        config.environment.horizontal_size_class = reported;
        let mut app = Headless::start(config).unwrap();
        app.first_render().unwrap();
        app.wait_for(&format!("text:width: {expected}")).unwrap();
    }
}
