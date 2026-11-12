//! Navigation and dialogs in Rust: the builders of TSX's `NavStack`,
//! `NavPage` and `Dialog` of `nsh/base`, so both faces import them from the
//! same layer, and of split views (Rust only for now). The elements are the
//! core's host elements ([`Stack`], [`Screen`], [`Modal`], [`SplitView`]);
//! these are where apps make them.

use nsh_core::element::{Modal, Screen, SplitView, Stack};

/// A native navigation stack (TSX: `NavStack`): its children are its
/// [`nav_page`]s, bottom to top. Put a page in to push it, leave it out to
/// pop it, and give each page a key (`Element::key`, its route) so the pages
/// below keep their state. UINavigationController on iOS (swipe back, the
/// system header), a stack with a toolbar and predictive back on Android,
/// the app's `header` on the desktops and the web.
pub fn nav_stack() -> Stack {
    nsh_core::element::stack()
}

/// One page of a [`nav_stack`] (TSX: `NavPage`): the page's content (its
/// children), its header on the desktops and the web, and the options of
/// the system header on iOS and Android. What it shows reads its insets
/// below the header (`Scope::insets`). `on_dismiss` tells the app the user
/// went back, for it to drop the route.
pub fn nav_page() -> Screen {
    nsh_core::element::screen()
}

/// A split view: two or three columns side by side, each a [`nav_stack`]
/// with its own pages and header, as an iPad's Mail shows a sidebar, a
/// list and a message. Give it a [`SplitView::primary`] column (a
/// sidebar), optionally a [`SplitView::supplementary`] one (a list) and a
/// [`SplitView::secondary`] one (the detail), each a nav stack.
///
/// `UISplitViewController` on iOS (column style, the system's sidebar
/// button and appearance): UIKit shows the columns that fit
/// ([`SplitView::preferred_display_mode`]) and reports changes
/// ([`SplitView::on_display_mode_change`]). Elsewhere a row of the
/// columns. A split view is for room a regular width gives: in a compact
/// width (`horizontal_size_class()`), show a phone's layout instead (a
/// `TabBar` of nav stacks); one left mounted there collapses as UIKit
/// does, its columns' pages onto one stack.
///
/// ```ignore
/// split_view()
///     .primary(nav_stack().child(nav_page().title("Mail").child(mailboxes())))
///     .supplementary(nav_stack().child(nav_page().title("Inbox").child(messages())))
///     .secondary(nav_stack().children(detail_pages()))
///     .preferred_primary_column_width(px(280.0))
///     .flex_1()
/// ```
pub fn split_view() -> SplitView {
    nsh_core::element::split_view()
}

/// A dialog (TSX: `Dialog`): presents its children over everything else,
/// the stack's header included, in its own native window layer. It has no
/// look of its own: what it shows (a popup, a backdrop) is the app's. Put it
/// in the view to present it (in a region: `If`), leave it out to dismiss
/// it; `on_cancel` tells the app the user tried to close it (Escape, the
/// Android back button, a press on the backdrop, a swipe down on an iOS
/// sheet).
pub fn dialog() -> Modal {
    nsh_core::element::modal()
}
