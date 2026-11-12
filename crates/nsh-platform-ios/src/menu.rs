//! `UIMenu`s of [`MenuItem`]s: the menus of trailing header bar buttons
//! ([`crate::navigation`]) and views' context menus
//! ([`crate::context_menu`]); the main menu ([`crate::main_menu`]) builds its
//! sections with the same [`menu_children`], its actions commands.
//!
//! An action with a handler carries the id the core gave it; choosing it
//! hands the id to the menu's [`Select`], which reports it as the root's
//! `Menu` event. An SF Symbol (`symbol`) is the image; `destructive`,
//! `enabled` and `checked` are the action's attributes and state; inline
//! submenus and the groups between separators are `UIMenu`s displayed
//! inline.

use std::{ptr::NonNull, rc::Rc};

use block2::RcBlock;
use nsh_core::desktop::{MenuAction, MenuItem};
use objc2::{MainThreadMarker, rc::Retained};
use objc2_foundation::{NSArray, NSString};
use objc2_ui_kit::{
    UIAction, UIImage, UIMenu, UIMenuElement, UIMenuElementAttributes, UIMenuElementState,
    UIMenuOptions,
};

/// What choosing an action does with its id (never 0).
pub(crate) type Select = Rc<dyn Fn(u32)>;

/// An SF Symbol image, or `None` when the name is unknown.
pub(crate) fn system_image(name: &str) -> Option<Retained<UIImage>> {
    UIImage::systemImageNamed(&NSString::from_str(name))
}

/// Builds a `UIMenu` tree from [`MenuItem`]s.
pub(crate) fn make_menu(
    mtm: MainThreadMarker,
    items: &[MenuItem],
    select: &Select,
) -> Retained<UIMenu> {
    let mut leaf =
        |action: &MenuAction| Retained::into_super(make_menu_action(mtm, action, select));
    let children = menu_children(mtm, items, &mut leaf, true);
    UIMenu::menuWithTitle_children(&NSString::from_str(""), &children, mtm)
}

/// Makes the element of an action, as it shows (`enabled` already folds in
/// the submenus around it).
pub(crate) type Leaf<'a> = dyn FnMut(&MenuAction) -> Retained<UIMenuElement> + 'a;

/// A `UIAction` for a [`MenuAction`].
fn make_menu_action(
    mtm: MainThreadMarker,
    action: &MenuAction,
    select: &Select,
) -> Retained<UIAction> {
    let title = NSString::from_str(&action.label);
    // Prefer an SF Symbol; `icon` (an image source) is for desktop menus.
    let image = action.symbol.as_deref().and_then(system_image);
    let id = action.id;
    let select = select.clone();
    let handler = RcBlock::new(move |_: NonNull<UIAction>| {
        if id != 0 {
            select(id);
        }
    });
    // No UIKit identifier: the API lets an app reuse an action id in two
    // menus, and UIKit may drop or merge elements that share one. The
    // handler already carries the id.
    // SAFETY: UIKit copies the handler; the block stays while the action does.
    let item = unsafe {
        UIAction::actionWithTitle_image_identifier_handler(
            &title,
            image.as_deref(),
            None,
            (&*handler as *const block2::DynBlock<_>).cast_mut(),
            mtm,
        )
    };
    let mut attrs = UIMenuElementAttributes::empty();
    if !action.enabled {
        attrs |= UIMenuElementAttributes::Disabled;
    }
    if action.destructive {
        attrs |= UIMenuElementAttributes::Destructive;
    }
    item.setAttributes(attrs);
    item.setState(if action.checked == Some(true) {
        UIMenuElementState::On
    } else {
        UIMenuElementState::Off
    });
    item
}

/// One menu element from a non-separator [`MenuItem`].
fn menu_element(
    mtm: MainThreadMarker,
    entry: &MenuItem,
    leaf: &mut Leaf<'_>,
    enabled: bool,
) -> Option<Retained<UIMenuElement>> {
    match entry {
        MenuItem::Separator => None,
        MenuItem::Action(action) if !enabled && action.enabled => Some(leaf(&MenuAction {
            enabled: false,
            ..action.clone()
        })),
        MenuItem::Action(action) => Some(leaf(action)),
        MenuItem::Submenu {
            label,
            enabled: submenu_enabled,
            items,
            icon: _,
            symbol,
            inline,
        } => {
            let child_enabled = enabled && *submenu_enabled;
            let nested = menu_children(mtm, items, leaf, child_enabled);
            let image = symbol.as_deref().and_then(system_image);
            let options = if *inline {
                UIMenuOptions::DisplayInline
            } else {
                UIMenuOptions::empty()
            };
            let submenu = UIMenu::menuWithTitle_image_identifier_options_children(
                &NSString::from_str(label),
                image.as_deref(),
                None,
                options,
                &nested,
                mtm,
            );
            Some(Retained::into_super(submenu))
        }
    }
}

/// Menu children for a list of [`MenuItem`]s. Separators split the list into
/// consecutive inline `UIMenu` groups (UIKit draws separators between those);
/// with no separators the elements stay flat. A disabled submenu has no
/// enabled state of its own, so its children are disabled instead. `leaf`
/// makes the actions' elements: `UIAction`s for the menus of buttons and
/// views, commands for the main menu ([`crate::main_menu`]).
pub(crate) fn menu_children(
    mtm: MainThreadMarker,
    entries: &[MenuItem],
    leaf: &mut Leaf<'_>,
    enabled: bool,
) -> Retained<NSArray<UIMenuElement>> {
    let has_separator = entries.iter().any(|e| matches!(e, MenuItem::Separator));
    if !has_separator {
        let children: Vec<_> = entries
            .iter()
            .filter_map(|entry| menu_element(mtm, entry, leaf, enabled))
            .collect();
        return NSArray::from_retained_slice(&children);
    }
    let mut children: Vec<Retained<UIMenuElement>> = Vec::new();
    let mut group: Vec<Retained<UIMenuElement>> = Vec::new();
    let flush = |group: &mut Vec<Retained<UIMenuElement>>,
                 children: &mut Vec<Retained<UIMenuElement>>| {
        if group.is_empty() {
            return;
        }
        let elems = NSArray::from_retained_slice(group);
        let section = UIMenu::menuWithTitle_image_identifier_options_children(
            &NSString::from_str(""),
            None,
            None,
            UIMenuOptions::DisplayInline,
            &elems,
            mtm,
        );
        children.push(Retained::into_super(section));
        group.clear();
    };
    for entry in entries {
        match entry {
            MenuItem::Separator => flush(&mut group, &mut children),
            other => {
                if let Some(el) = menu_element(mtm, other, leaf, enabled) {
                    group.push(el);
                }
            }
        }
    }
    flush(&mut group, &mut children);
    NSArray::from_retained_slice(&children)
}
