//! Views' context menus ([`super::Interactive::context_menu`]).
//!
//! A binding's [`desktop::MenuItem`]s are decoded into the platform's
//! [`crate::desktop::MenuItem`]s with an id per item that has a handler, as
//! the header's menus are ([`super::header`]); the handlers stay in
//! [`super::menus::ViewMenus`] under a generation that a cleanup drops when
//! the binding runs again or the view goes. The platform reports a chosen
//! item's id as the root's [`crate::EventKind::Menu`], and the core runs its
//! handler.

use std::rc::Rc;

use super::{cleanup, desktop, reactive::Owner, view_menus};

crate::static_reactive!(Vec<desktop::MenuItem>: Vec<desktop::MenuItem>);

/// Decodes `items` into the platform's form, keeps their handlers until
/// this binding is replaced or its owner goes, and returns what
/// [`crate::ViewProps::context_menu`] holds.
pub(crate) fn register(items: Vec<desktop::MenuItem>) -> Vec<crate::desktop::MenuItem> {
    if items.is_empty() {
        return Vec::new();
    }
    let menus = view_menus();
    let owner = Owner::current();
    let mut handlers = Vec::new();
    let mac = super::os() == crate::platform::Os::MacOs;
    let decoded = desktop::decode_for_header(items, mac, &owner, &mut handlers);
    if handlers.is_empty() {
        return decoded;
    }
    let generation = menus.borrow_mut().push_context(handlers);
    let weak = Rc::downgrade(&menus);
    cleanup(move || {
        if let Some(menus) = weak.upgrade() {
            let old = menus.borrow_mut().take_context(generation);
            drop(old);
        }
    });
    decoded
}
