//! Split view (`nsh_base::split_view`): a mail client's three columns, as
//! an iPad shows them. The sidebar lists the mailboxes, the list column
//! the chosen mailbox's messages, the detail column the chosen message,
//! and the detail pushes its sender's card. Each column is a nav stack
//! with its own header and bar buttons. On iOS UIKit shows the columns
//! that fit (three in landscape, the list and the message in portrait, the
//! sidebar a button away); elsewhere the columns sit in a row.
//!
//! It takes the whole window: the gallery's demo page opens it
//! ([`launcher`]) and its sidebar's close button leaves it, so the split
//! view mounts and unmounts as an app's root layout does when the width
//! changes class.

use nsh_base::{nav_page, nav_stack, split_view};
use nsh_core::{
    ScreenAnimation,
    element::{AnyElement, ImageSrc, Screen, desktop::MenuItem, header::HeaderItem},
    platform::Os,
    prelude::*,
};

use crate::ui::{Button, Card, colors, radius, tints};

/// Whether the split view takes the window (a global of the surface's).
#[derive(Default, Clone, Copy, PartialEq)]
pub(crate) struct SplitOpen(pub(crate) bool);

struct Mailbox {
    name: &'static str,
    symbol: &'static str,
}

const MAILBOXES: &[Mailbox] = &[
    Mailbox {
        name: "Inbox",
        symbol: "tray",
    },
    Mailbox {
        name: "Flagged",
        symbol: "flag",
    },
    Mailbox {
        name: "Sent",
        symbol: "paperplane",
    },
    Mailbox {
        name: "Archive",
        symbol: "archivebox",
    },
];

struct Message {
    id: u32,
    mailbox: usize,
    from: &'static str,
    email: &'static str,
    subject: &'static str,
    preview: &'static str,
    time: &'static str,
    unread: bool,
}

const MESSAGES: &[Message] = &[
    Message {
        id: 1,
        mailbox: 0,
        from: "Ava Lindqvist",
        email: "ava@northwind.io",
        subject: "Launch checklist for Thursday",
        preview: "I went through the list with design this morning. Two items are still open: the release notes and the store screenshots.",
        time: "9:41",
        unread: true,
    },
    Message {
        id: 2,
        mailbox: 0,
        from: "Mateo Ruiz",
        email: "mateo@fieldnotes.app",
        subject: "Re: Offline sync",
        preview: "The conflict screen reads well now. One question about the retry button: should it keep the user's edits?",
        time: "8:15",
        unread: true,
    },
    Message {
        id: 3,
        mailbox: 0,
        from: "Hana Sato",
        email: "hana@kumo.studio",
        subject: "Photos from the offsite",
        preview: "Uploaded everything to the shared album. The sunset ones by the lake came out especially well.",
        time: "Yesterday",
        unread: false,
    },
    Message {
        id: 4,
        mailbox: 0,
        from: "Billing",
        email: "billing@cloudnine.dev",
        subject: "Your November invoice",
        preview: "Thanks for being a customer. Your invoice for November is ready and will be charged on the 1st.",
        time: "Monday",
        unread: false,
    },
    Message {
        id: 5,
        mailbox: 1,
        from: "Jonas Weber",
        email: "jonas@tidewater.co",
        subject: "Contract draft v3",
        preview: "Legal signed off on section four. Could you look at the renewal terms before Friday?",
        time: "Tuesday",
        unread: false,
    },
    Message {
        id: 6,
        mailbox: 1,
        from: "Ava Lindqvist",
        email: "ava@northwind.io",
        subject: "Roadmap review",
        preview: "Moved the review to next week so the numbers from the beta are in. Agenda attached.",
        time: "Oct 28",
        unread: false,
    },
    Message {
        id: 7,
        mailbox: 2,
        from: "Mateo Ruiz",
        email: "mateo@fieldnotes.app",
        subject: "Offline sync",
        preview: "Here is the build with the new conflict screen. Let me know what you think of the copy.",
        time: "Yesterday",
        unread: false,
    },
    Message {
        id: 8,
        mailbox: 3,
        from: "Hana Sato",
        email: "hana@kumo.studio",
        subject: "Venue options",
        preview: "Three places by the lake have room for all of us. The second one has the best kitchen.",
        time: "Oct 12",
        unread: false,
    },
];

fn message(id: u32) -> Option<&'static Message> {
    MESSAGES.iter().find(|m| m.id == id)
}

fn first_of(mailbox: usize) -> Option<u32> {
    MESSAGES.iter().find(|m| m.mailbox == mailbox).map(|m| m.id)
}

fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .collect()
}

/// A quiet line of text.
fn note(content: impl IntoReactive<SharedString>) -> AnyElement {
    text(content)
        .text_size(px(13.0))
        .line_height(px(18.0))
        .text_color(colors::MUTED)
        .into_element()
}

/// The platform draws no header of its own (the desktops, the web): the
/// pages carry theirs.
fn draws_header() -> bool {
    !matches!(os(), Os::Ios | Os::Android)
}

/// A column's header where the app draws it: an optional back button, the
/// title and the page's actions.
fn column_header(
    title: impl IntoReactive<SharedString>,
    back: Option<Box<dyn Fn()>>,
    actions: Vec<AnyElement>,
) -> AnyElement {
    view()
        .h(px(52.0))
        .flex_row()
        .items_center()
        .gap_2()
        .px_4()
        .bg(colors::BACKGROUND)
        .border_b_1()
        .border_b_color(colors::LINE)
        .children(back.map(|back| {
            text("Back")
                .text_size(px(15.0))
                .text_color(colors::ACCENT)
                .role(AccessibilityRole::Button)
                .on_press(back)
        }))
        .child(
            text(title)
                .flex_1()
                .number_of_lines(1)
                .text_size(px(17.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors::TEXT),
        )
        .children(actions)
        .into_element()
}

/// A text button of a drawn header.
fn header_action(label: &'static str, f: impl Fn() + 'static) -> AnyElement {
    text(label)
        .text_size(px(15.0))
        .text_color(colors::ACCENT)
        .role(AccessibilityRole::Button)
        .active(|s| s.opacity(0.6))
        .on_press(f)
        .into_element()
}

/// The mail client: a sidebar of mailboxes, their messages, a message.
#[component]
pub(crate) fn MailSplit() -> impl IntoElement {
    let mailbox = signal(0usize);
    let selected = signal(first_of(0));
    let contact = signal(false);
    let flagged = signal(vec![5u32, 6]);
    let by_sender = signal(false);
    let mode = signal(SplitDisplayMode::Automatic);
    let split = element_ref();

    let detail = nav_stack()
        .flex_1()
        .child(dynamic(move || {
            let id = selected.get();
            detail_page(id, contact, flagged, split).into_element()
        }))
        .child(dynamic(move || {
            let id = selected.get()?;
            if contact.get() {
                contact_page(id, contact)
            } else {
                None
            }
        }));

    split_view()
        .element_ref(&split)
        .primary(
            nav_stack()
                .flex_1()
                .child(mailboxes_page(mailbox, selected, contact, mode)),
        )
        .supplementary(
            nav_stack()
                .flex_1()
                .child(messages_page(mailbox, selected, contact, by_sender)),
        )
        .secondary(detail)
        .preferred_primary_column_width(px(260.0))
        .preferred_supplementary_column_width(px(360.0))
        .min_supplementary_column_width(px(320.0))
        .on_display_mode_change(move |m: &SplitDisplayMode| mode.set(*m))
        .flex_1()
}

/// Closes the split view: the gallery shows again.
fn close() {
    global::<SplitOpen>().set(SplitOpen(false));
}

/// The sidebar's page: the mailboxes, the chosen one marked.
fn mailboxes_page(
    mailbox: Signal<usize>,
    selected: Signal<Option<u32>>,
    contact: Signal<bool>,
    mode: Signal<SplitDisplayMode>,
) -> Screen {
    let rows = MAILBOXES.iter().enumerate().map(move |(i, b)| {
        let active = move || mailbox.get() == i;
        let count = MESSAGES.iter().filter(|m| m.mailbox == i).count();
        view()
            .h(px(44.0))
            .flex_row()
            .items_center()
            .gap_3()
            .px_3()
            .rounded(px(radius::CONTROL))
            .role(AccessibilityRole::Button)
            .label(b.name)
            .accessibility_state(move || AccessibilityState {
                selected: active(),
                ..AccessibilityState::default()
            })
            .style_when(active, |s| s.bg(tints::ACCENT))
            .active(|s| s.bg(tints::ACCENT))
            .on_press(move || {
                mailbox.set(i);
                contact.set(false);
                selected.set(first_of(i));
            })
            .when(matches!(os(), Os::Ios), |row| {
                row.child(
                    image(ImageSrc::symbol(b.symbol))
                        .size(px(20.0))
                        .tint_color(colors::ACCENT),
                )
            })
            .child(
                text(b.name)
                    .flex_1()
                    .text_size(px(16.0))
                    .text_color(colors::TEXT)
                    .style_when(active, |s| s.font_weight(FontWeight::SEMIBOLD)),
            )
            .child(
                text(count.to_string())
                    .text_size(px(14.0))
                    .text_color(colors::MUTED),
            )
    });
    let page = nav_page()
        .title("Mailboxes")
        .header_large_title(true)
        .header_right_items(vec![
            HeaderItem::symbol("xmark")
                .label("Close")
                .key("close")
                .on_press(close),
        ])
        .child(
            scroll().flex_1().child(
                // iPadOS's floating sidebar covers the column beside it: the
                // pages keep to their safe area.
                view().px_safe_area().child(
                    view()
                        .px_3()
                        .pt_2()
                        .pb_6()
                        .gap_1()
                        .children(rows)
                        .child(view().h_4())
                        .child(note(move || {
                            let width = match horizontal_size_class() {
                                SizeClass::Regular => "Regular width",
                                SizeClass::Compact => "Compact width",
                                SizeClass::Unspecified => "Width not known yet",
                            };
                            let columns = match mode.get() {
                                SplitDisplayMode::Automatic => "the columns that fit",
                                SplitDisplayMode::SecondaryOnly => "the message alone",
                                SplitDisplayMode::OneBesideSecondary => {
                                    "the list beside the message"
                                }
                                SplitDisplayMode::OneOverSecondary => "the list over the message",
                                SplitDisplayMode::TwoBesideSecondary => {
                                    "three columns side by side"
                                }
                                SplitDisplayMode::TwoOverSecondary => "the sidebar over the list",
                                SplitDisplayMode::TwoDisplaceSecondary => {
                                    "the sidebar pushing the message aside"
                                }
                            };
                            format!("{width}, {columns}")
                        })),
                ),
            ),
        );
    if draws_header() {
        page.header(column_header(
            "Mailboxes",
            None,
            vec![header_action("Close", close)],
        ))
    } else {
        page
    }
}

/// The list's page: the chosen mailbox's messages, newest or by sender;
/// a press shows one in the detail column.
fn messages_page(
    mailbox: Signal<usize>,
    selected: Signal<Option<u32>>,
    contact: Signal<bool>,
    by_sender: Signal<bool>,
) -> Screen {
    let title = move || SharedString::from(MAILBOXES[mailbox.get()].name);
    let list = dynamic(move || {
        let mut messages: Vec<&'static Message> = MESSAGES
            .iter()
            .filter(|m| m.mailbox == mailbox.get())
            .collect();
        if by_sender.get() {
            messages.sort_by_key(|m| m.from);
        }
        messages
            .into_iter()
            .map(|m| message_row(m, selected, contact))
            .collect::<Vec<_>>()
    });
    let page = nav_page()
        .title(title)
        .header_large_title(true)
        .header_right_items(move || {
            let sender = by_sender.get();
            vec![
                HeaderItem::symbol("line.3.horizontal.decrease.circle")
                    .label("Sort")
                    .key("sort")
                    .menu([
                        MenuItem::new("Newest First")
                            .symbol("clock")
                            .checked(!sender)
                            .on_select(move || by_sender.set(false)),
                        MenuItem::new("By Sender")
                            .symbol("person")
                            .checked(sender)
                            .on_select(move || by_sender.set(true)),
                    ]),
            ]
        })
        .child(
            scroll().flex_1().child(
                view()
                    .px_safe_area()
                    .child(view().px_3().pt_1().pb_6().gap_1().child(list)),
            ),
        );
    if draws_header() {
        page.header(column_header(
            title,
            None,
            vec![header_action("Sort", move || {
                by_sender.update(|s| *s = !*s)
            })],
        ))
    } else {
        page
    }
}

fn message_row(
    m: &'static Message,
    selected: Signal<Option<u32>>,
    contact: Signal<bool>,
) -> AnyElement {
    let active = move || selected.get() == Some(m.id);
    view()
        .flex_row()
        .gap_3()
        .px_3()
        .py_3()
        .rounded(px(radius::INSET))
        .role(AccessibilityRole::Button)
        .label(m.subject)
        .style_when(active, |s| s.bg(tints::ACCENT))
        .active(|s| s.bg(tints::ACCENT))
        .on_press(move || {
            contact.set(false);
            selected.set(Some(m.id));
        })
        .child(
            view().w(px(8.0)).pt(px(6.0)).child(
                view()
                    .size(px(8.0))
                    .rounded_full()
                    .bg(colors::ACCENT)
                    .opacity(if m.unread { 1.0_f32 } else { 0.0 }),
            ),
        )
        .child(
            view()
                .flex_1()
                .gap(px(3.0))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(
                            text(m.from)
                                .flex_1()
                                .number_of_lines(1)
                                .text_size(px(15.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(colors::TEXT),
                        )
                        .child(text(m.time).text_size(px(13.0)).text_color(colors::MUTED)),
                )
                .child(
                    text(m.subject)
                        .number_of_lines(1)
                        .text_size(px(14.0))
                        .text_color(colors::TEXT),
                )
                .child(
                    text(m.preview)
                        .number_of_lines(2)
                        .text_size(px(14.0))
                        .line_height(px(19.0))
                        .text_color(colors::MUTED),
                ),
        )
        .into_element()
}

/// A round badge of a sender's initials.
fn avatar(name: &str, size: f32) -> AnyElement {
    view()
        .size(px(size))
        .rounded_full()
        .items_center()
        .justify_center()
        .bg(colors::ACCENT_FILL)
        .child(
            text(initials(name))
                .text_size(px(size * 0.38))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors::ON_ACCENT),
        )
        .into_element()
}

/// The detail column's root page: the chosen message, keyed by it so a
/// new choice replaces it (and the pages over it).
fn detail_page(
    id: Option<u32>,
    contact: Signal<bool>,
    flagged: Signal<Vec<u32>>,
    split: ElementRef,
) -> Screen {
    let key = id.map_or_else(|| "message:none".to_owned(), |id| format!("message:{id}"));
    let page = nav_page()
        .key(key)
        .title("")
        .animation(ScreenAnimation::Fade)
        .header_back_title("Message");
    let Some(m) = id.and_then(message) else {
        return page.child(
            view()
                .flex_1()
                .items_center()
                .justify_center()
                .child(note("No message selected")),
        );
    };
    let is_flagged = move || flagged.with(|f| f.contains(&m.id));
    let toggle_flag = move || {
        flagged.update(|f| match f.iter().position(|&x| x == m.id) {
            Some(at) => {
                f.remove(at);
            }
            None => f.push(m.id),
        })
    };
    let page = page
        .header_right_items(move || {
            vec![
                HeaderItem::symbol("ellipsis.circle")
                    .label("More")
                    .key("more")
                    .menu([
                        MenuItem::new("Show Mailboxes")
                            .symbol("sidebar.left")
                            .on_select(move || split.show_column(SplitColumn::Primary)),
                        MenuItem::new("Focus on Message")
                            .symbol("arrow.up.left.and.arrow.down.right")
                            .on_select(move || split.hide_column(SplitColumn::Supplementary)),
                    ]),
                HeaderItem::symbol(if is_flagged() { "flag.fill" } else { "flag" })
                    .label(if is_flagged() { "Unflag" } else { "Flag" })
                    .key("flag")
                    .on_press(toggle_flag),
            ]
        })
        .child(
            scroll().flex_1().child(
                view().px_safe_area().child(
                    view()
                        .px_6()
                        .pt_4()
                        .pb_10()
                        .gap_5()
                        .max_w(px(720.0))
                        .child(
                            text(m.subject)
                                .text_size(px(24.0))
                                .line_height(px(30.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(colors::TEXT),
                        )
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap_3()
                                .p_3()
                                .rounded(px(radius::CARD))
                                .bg(colors::INSET)
                                .role(AccessibilityRole::Button)
                                .label(format!("From {}", m.from))
                                .active(|s| s.bg(colors::RAISED))
                                .on_press(move || contact.set(true))
                                .child(avatar(m.from, 40.0))
                                .child(
                                    view()
                                        .flex_1()
                                        .gap(px(2.0))
                                        .child(
                                            text(m.from)
                                                .text_size(px(15.0))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(colors::TEXT),
                                        )
                                        .child(
                                            text(m.email)
                                                .text_size(px(13.0))
                                                .text_color(colors::MUTED),
                                        ),
                                )
                                .child(text(m.time).text_size(px(13.0)).text_color(colors::MUTED)),
                        )
                        .child(
                            text(m.preview)
                                .text_size(px(16.0))
                                .line_height(px(24.0))
                                .text_color(colors::TEXT),
                        )
                        .child(
                            text(
                                "Let me know if anything in here needs another pass before we \
                             share it more widely. Happy to walk through it on a call.",
                            )
                            .text_size(px(16.0))
                            .line_height(px(24.0))
                            .text_color(colors::TEXT),
                        )
                        .child(
                            text(m.from.split_whitespace().next().unwrap_or(m.from))
                                .text_size(px(16.0))
                                .text_color(colors::MUTED),
                        ),
                ),
            ),
        );
    if draws_header() {
        page.header(column_header(
            m.subject,
            None,
            vec![header_action("Flag", toggle_flag)],
        ))
    } else {
        page
    }
}

/// The sender's card, pushed over the message.
fn contact_page(id: u32, contact: Signal<bool>) -> Option<Screen> {
    let m = message(id)?;
    let sent = MESSAGES.iter().filter(|x| x.from == m.from).count();
    let page = nav_page()
        .key(format!("contact:{id}"))
        .title(m.from)
        .on_dismiss(move || contact.set(false))
        .child(
            scroll().flex_1().child(
                view()
                    .items_center()
                    .px_6()
                    .pt_8()
                    .pb_10()
                    .gap_3()
                    .child(avatar(m.from, 88.0))
                    .child(
                        text(m.from)
                            .text_size(px(24.0))
                            .font_weight(FontWeight::BOLD)
                            .text_color(colors::TEXT),
                    )
                    .child(text(m.email).text_size(px(15.0)).text_color(colors::ACCENT))
                    .child(note(format!(
                        "{sent} conversation{} in these mailboxes",
                        if sent == 1 { "" } else { "s" }
                    ))),
            ),
        );
    Some(if draws_header() {
        page.header(column_header(
            m.from,
            Some(Box::new(move || contact.set(false))),
            Vec::new(),
        ))
    } else {
        page
    })
}

/// The demo's page in the gallery: what a split view is, and the button
/// that gives it the window.
pub(crate) fn launcher() -> impl IntoElement {
    scroll().flex_1().child(
        view().p_4().gap_4().child(
            Card::new()
                .child(
                    text(
                        "Three columns, each a navigation stack with its own header: \
                         mailboxes, their messages and a message, which pushes its \
                         sender. On an iPad UIKit shows the columns that fit and the \
                         sidebar button reveals the rest.",
                    )
                    .text_size(px(15.0))
                    .line_height(px(21.0))
                    .text_color(colors::MUTED),
                )
                .child(
                    view()
                        .pt_3()
                        .child(Button::new("Open split view", |_: &()| {
                            global::<SplitOpen>().set(SplitOpen(true))
                        })),
                ),
        ),
    )
}
