//! The application menu (`set_application_menu`, TS `Menu.setApplicationMenu`)
//! as UIKit's main menu: the iPad's menu bar (iPadOS 26), the shortcuts a
//! held Command key lists (iPadOS 15) and Mac Catalyst's menu bar.
//!
//! The menu the app sets is kept here and the main menu system is asked to
//! rebuild; the standalone app's delegate ([`crate::app`]) builds it
//! ([`build`]) and carries out its commands ([`can_choose`], [`choose`]).
//! UIKit's own menus stay (the application menu, File, Edit, Format, View,
//! Window, Help) and the app's are added to them:
//!
//! - A top-level submenu goes into the standard menu its role items belong
//!   to (About, Hide, Quit: the application menu; Close: File; Undo to
//!   Select All: Edit; Minimize, Zoom, Full Screen: Window), else into the
//!   standard menu of its label (`File`, `Edit`, `Format`, `View`,
//!   `Window`, `Help`, or the title UIKit gives that menu), as an inline
//!   section at its end (after About in the application menu). Any other
//!   is a menu of its own, between View and Window, in the app's order. A
//!   top-level action is a menu of its own, as on macOS.
//! - Role items are left out: UIKit's standard menus have the system's
//!   own (Undo, Copy, Close Window, Hide, Quit).
//! - An action with a shortcut is a `UIKeyCommand`, titled, so it shows in
//!   the menus and the shortcut list and its key chooses it; the others are
//!   `UICommand`s. Separators and inline submenus are inline sections, as in
//!   the other menus ([`crate::menu`]); symbols, checks, disabled and
//!   destructive items show as there.
//! - UIKit drops a whole inserted menu when one of its shortcuts is in the
//!   main menu already, so the app's shortcut wins: UIKit's own command of
//!   it is removed (Command-F takes Find's), and of two items of the app's
//!   with one shortcut, the later one shows without it.
//!
//! The menu's shortcuts are UIKit's key commands alone, never entries of
//! the app's keymap ([`crate::keymap`]), so a key chooses an item once.
//! Presses reach the responders before UIKit matches key commands, so the
//! keymap comes first, as on the desktops: a keystroke `nsh/keymap` binds
//! fires its binding and the menu's item of it is not chosen. A command can
//! be chosen ([`can_choose`]) while its item is enabled and, with a text
//! input focused, fires there ([`MenuAction::fires_while_editing`]); else
//! UIKit shows it disabled and the key goes on to the field.
//!
//! Only the standalone app ([`crate::app`]) builds the main menu: an app
//! embedding `AppView` owns its delegate, so the menu set there does
//! nothing yet.

use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    sync::Arc,
};

use nsh_core::{
    EventKind, EventPayload, PayloadValue, ROOT_ID,
    desktop::{Key, MenuAction, MenuItem, MenuRole, Shortcut},
};
use nsh_engine::EngineControl;
use objc2::{
    MainThreadMarker, Message,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject, Sel},
    sel,
};
use objc2_foundation::{NSNumber, NSString};
use objc2_ui_kit::{
    UIApplication, UICommand, UIKeyCommand, UIKeyModifierFlags, UIMenu, UIMenuBuilder,
    UIMenuElement, UIMenuElementAttributes, UIMenuElementState, UIMenuIdentifier, UIMenuOptions,
    UIMenuSystem, UIResponder, UIView, UIWindowScene,
};

/// The menu the app set and the engine its choices go to.
struct AppMenu {
    items: Arc<[MenuItem]>,
    control: EngineControl,
}

/// A command of the main menu as built: the item it chooses and when it
/// can be chosen. Its `propertyList` is its index, with the build's
/// generation: a command of an earlier build does nothing.
struct Entry {
    id: u32,
    enabled: bool,
    while_editing: bool,
}

thread_local! {
    static MENU: RefCell<Option<AppMenu>> = const { RefCell::new(None) };
    static ENTRIES: RefCell<Vec<Entry>> = const { RefCell::new(Vec::new()) };
    /// Counts the builds, to tell the commands of the last one.
    static GENERATION: Cell<u32> = const { Cell::new(0) };
}

/// The action of the main menu's commands, which the app delegate has.
pub(crate) fn action() -> Sel {
    sel!(nshMainMenuCommand:)
}

/// `DesktopCommand::SetMenu`: the app's menu, chosen items reported to
/// `control` as the root's `Menu` event, or `None` for UIKit's own alone.
pub(crate) fn set(mtm: MainThreadMarker, items: Option<Arc<[MenuItem]>>, control: &EngineControl) {
    let menu = items.map(|items| AppMenu {
        items,
        control: control.clone(),
    });
    MENU.with(|m| *m.borrow_mut() = menu);
    UIMenuSystem::mainSystem(mtm).setNeedsRebuild();
}

/// `buildMenuWithBuilder:` of the app delegate: adds the app's menu to the
/// main menu UIKit built.
pub(crate) fn build(mtm: MainThreadMarker, builder: &ProtocolObject<dyn UIMenuBuilder>) {
    if builder.system() != UIMenuSystem::mainSystem(mtm) {
        return;
    }
    let generation = GENERATION.with(|g| {
        g.set(g.get().wrapping_add(1));
        g.get()
    });
    let mut entries = Vec::new();
    let items = MENU.with(|m| m.borrow().as_ref().map(|m| m.items.clone()));
    let items = items.as_deref().unwrap_or_default();
    // UIKit drops a whole menu inserted with a shortcut the main menu has
    // already: the app's take theirs from UIKit's own commands, and an
    // item of the app's whose shortcut an earlier one has shows without it.
    let mut wanted = HashSet::new();
    MenuItem::each_action(items, &mut |action| {
        if action.role.is_none()
            && let Some(key) = action.shortcut.as_ref().and_then(key_input)
        {
            wanted.insert(key_id(&key));
        }
    });
    if !wanted.is_empty()
        && let Some(root) = builder.menuForIdentifier(unsafe { objc2_ui_kit::UIMenuRoot })
    {
        release(builder, &root, &wanted);
    }
    let mut used = HashSet::new();
    // The app's menus of their own, after one another.
    let mut last_own: Option<Retained<NSString>> = None;
    for (n, item) in items.iter().enumerate() {
        let (label, enabled, children) = match item {
            MenuItem::Submenu {
                label,
                enabled,
                items,
                ..
            } => (label.as_str(), *enabled, items.as_slice()),
            MenuItem::Action(action) => (action.label.as_str(), true, std::slice::from_ref(item)),
            MenuItem::Separator => continue,
        };
        let standard = standard_menu(builder, children, label);
        let children = without_roles(children);
        if children.is_empty() {
            continue;
        }
        let mut leaf = |action: &MenuAction| {
            let index = entries.len();
            entries.push(Entry {
                id: action.id,
                enabled: action.enabled,
                while_editing: action.fires_while_editing(),
            });
            command(mtm, action, generation, index, &mut used)
        };
        let elements = crate::menu::menu_children(mtm, &children, &mut leaf, enabled);
        let identifier = NSString::from_str(&format!("dev.nsh.application-menu.{n}"));
        let menu = |title: &str, options| {
            UIMenu::menuWithTitle_image_identifier_options_children(
                &NSString::from_str(title),
                None,
                Some(&identifier),
                options,
                &elements,
                mtm,
            )
        };
        match standard {
            // SAFETY (all): reading immutable UIKit constants.
            Some(parent) if parent == unsafe { objc2_ui_kit::UIMenuApplication } => {
                let section = menu("", UIMenuOptions::DisplayInline);
                let about = unsafe { objc2_ui_kit::UIMenuAbout };
                if builder.menuForIdentifier(about).is_some() {
                    builder.insertSiblingMenu_afterMenuForIdentifier(&section, about);
                } else {
                    builder.insertChildMenu_atStartOfMenuForIdentifier(&section, parent);
                }
            }
            Some(parent) => {
                let section = menu("", UIMenuOptions::DisplayInline);
                builder.insertChildMenu_atEndOfMenuForIdentifier(&section, parent);
            }
            None => {
                let own = menu(label, UIMenuOptions::empty());
                let before = [unsafe { objc2_ui_kit::UIMenuWindow }, unsafe {
                    objc2_ui_kit::UIMenuHelp
                }]
                .into_iter()
                .find(|id| builder.menuForIdentifier(id).is_some());
                match (&last_own, before) {
                    (Some(last), _) => builder.insertSiblingMenu_afterMenuForIdentifier(&own, last),
                    (None, Some(before)) => {
                        builder.insertSiblingMenu_beforeMenuForIdentifier(&own, before)
                    }
                    (None, None) => builder
                        .insertChildMenu_atEndOfMenuForIdentifier(&own, unsafe {
                            objc2_ui_kit::UIMenuRoot
                        }),
                }
                last_own = Some(identifier.clone());
            }
        }
    }
    ENTRIES.with(|e| *e.borrow_mut() = entries);
}

/// The standard menu that a top-level menu of `items` titled `label` goes
/// into, if the builder has it: the one of its first role item, else the
/// one of its label.
fn standard_menu(
    builder: &ProtocolObject<dyn UIMenuBuilder>,
    items: &[MenuItem],
    label: &str,
) -> Option<&'static UIMenuIdentifier> {
    // SAFETY (all): reading immutable UIKit constants.
    let mut role = None;
    MenuItem::each_action(items, &mut |action| role = role.or(action.role));
    let by_role = role.map(|role| match role {
        MenuRole::About
        | MenuRole::Hide
        | MenuRole::HideOthers
        | MenuRole::ShowAll
        | MenuRole::Quit => unsafe { objc2_ui_kit::UIMenuApplication },
        MenuRole::Close => unsafe { objc2_ui_kit::UIMenuFile },
        MenuRole::Undo
        | MenuRole::Redo
        | MenuRole::Cut
        | MenuRole::Copy
        | MenuRole::Paste
        | MenuRole::SelectAll => unsafe { objc2_ui_kit::UIMenuEdit },
        MenuRole::Minimize | MenuRole::Zoom | MenuRole::ToggleFullScreen => unsafe {
            objc2_ui_kit::UIMenuWindow
        },
    });
    if let Some(id) = by_role {
        return builder.menuForIdentifier(id).is_some().then_some(id);
    }
    let named = unsafe {
        [
            (objc2_ui_kit::UIMenuFile, "File"),
            (objc2_ui_kit::UIMenuEdit, "Edit"),
            (objc2_ui_kit::UIMenuFormat, "Format"),
            (objc2_ui_kit::UIMenuView, "View"),
            (objc2_ui_kit::UIMenuWindow, "Window"),
            (objc2_ui_kit::UIMenuHelp, "Help"),
            (objc2_ui_kit::UIMenuApplication, ""),
        ]
    };
    named.into_iter().find_map(|(id, english)| {
        let menu = builder.menuForIdentifier(id)?;
        let same = |title: &str| !title.is_empty() && title.eq_ignore_ascii_case(label);
        (same(english) || same(&menu.title().to_string())).then_some(id)
    })
}

/// `items` without their role items, and without the submenus that leaves
/// empty.
fn without_roles(items: &[MenuItem]) -> Vec<MenuItem> {
    items
        .iter()
        .filter_map(|item| match item {
            MenuItem::Action(MenuAction { role: Some(_), .. }) => None,
            MenuItem::Submenu {
                label,
                enabled,
                items,
                icon,
                symbol,
                inline,
            } => {
                let items = without_roles(items);
                (!items.is_empty()).then(|| MenuItem::Submenu {
                    label: label.clone(),
                    enabled: *enabled,
                    items,
                    icon: icon.clone(),
                    symbol: symbol.clone(),
                    inline: *inline,
                })
            }
            other => Some(other.clone()),
        })
        .collect()
}

/// The command of `item`, the `index`th of build `generation`.
fn command(
    mtm: MainThreadMarker,
    item: &MenuAction,
    generation: u32,
    index: usize,
    used: &mut HashSet<KeyId>,
) -> Retained<UIMenuElement> {
    let title = NSString::from_str(&item.label);
    let image = item.symbol.as_deref().and_then(crate::menu::system_image);
    let list = NSNumber::new_u64(u64::from(generation) << 32 | index as u64);
    let key = item
        .shortcut
        .as_ref()
        .and_then(key_input)
        .filter(|key| used.insert(key_id(key)));
    let command: Retained<UICommand> = match key {
        Some((input, flags)) => {
            // SAFETY: the app delegate has the action
            // (`nshMainMenuCommand:`); the property list is an NSNumber.
            let command = unsafe {
                UIKeyCommand::commandWithTitle_image_action_input_modifierFlags_propertyList(
                    &title,
                    image.as_deref(),
                    action(),
                    &input,
                    flags,
                    Some(&list),
                    mtm,
                )
            };
            // Whether the key goes to a focused text field is the item's
            // `when` ([`can_choose`]), not UIKit's: arrows, Return or Escape
            // choose it over scrolling or moving the focus too.
            command.setWantsPriorityOverSystemBehavior(true);
            Retained::into_super(command)
        }
        // SAFETY: as above.
        None => unsafe {
            UICommand::commandWithTitle_image_action_propertyList(
                &title,
                image.as_deref(),
                action(),
                Some(&list),
                mtm,
            )
        },
    };
    let mut attributes = UIMenuElementAttributes::empty();
    if !item.enabled {
        attributes |= UIMenuElementAttributes::Disabled;
    }
    if item.destructive {
        attributes |= UIMenuElementAttributes::Destructive;
    }
    command.setAttributes(attributes);
    if item.checked == Some(true) {
        command.setState(UIMenuElementState::On);
    }
    Retained::into_super(command)
}

/// A keyboard shortcut as UIKit tells shortcuts apart: the input in lower
/// case, and the modifier flags.
type KeyId = (String, isize);

fn key_id((input, flags): &(Retained<NSString>, UIKeyModifierFlags)) -> KeyId {
    (input.to_string().to_lowercase(), flags.0)
}

/// Removes UIKit's own key commands of `menu` (and its submenus) whose
/// shortcuts are `wanted`.
fn release(builder: &ProtocolObject<dyn UIMenuBuilder>, menu: &UIMenu, wanted: &HashSet<KeyId>) {
    for child in menu.children().iter() {
        if let Some(submenu) = child.downcast_ref::<UIMenu>() {
            release(builder, submenu, wanted);
            continue;
        }
        let Some(command) = child.downcast_ref::<UIKeyCommand>() else {
            continue;
        };
        let Some(input) = command.input() else {
            continue;
        };
        // SAFETY: the command's own action and property list identify it.
        unsafe {
            if let Some(action) = command.action()
                && wanted.contains(&key_id(&(input, command.modifierFlags())))
            {
                builder
                    .removeCommandForAction_propertyList(action, command.propertyList().as_deref());
            }
        }
    }
}

/// The input and modifier flags of a `UIKeyCommand` for `shortcut`; `None`
/// for a key UIKit names no input for (F13 to F24), whose item shows with
/// no shortcut.
fn key_input(shortcut: &Shortcut) -> Option<(Retained<NSString>, UIKeyModifierFlags)> {
    // The development host's Command-R reloads, whatever the app binds
    // (`crate::keymap`).
    if cfg!(feature = "dev")
        && shortcut.key == Key::Char('r')
        && shortcut.meta
        && !(shortcut.ctrl || shortcut.alt || shortcut.shift)
    {
        return None;
    }
    // SAFETY (all): reading immutable UIKit constants.
    let input = unsafe {
        match shortcut.key {
            Key::Char(c) => NSString::from_str(&c.to_string()),
            Key::Space => NSString::from_str(" "),
            Key::Enter => NSString::from_str("\r"),
            Key::Tab => NSString::from_str("\t"),
            Key::Backspace => NSString::from_str("\u{8}"),
            Key::Escape => objc2_ui_kit::UIKeyInputEscape.retain(),
            Key::Delete => objc2_ui_kit::UIKeyInputDelete.retain(),
            Key::Up => objc2_ui_kit::UIKeyInputUpArrow.retain(),
            Key::Down => objc2_ui_kit::UIKeyInputDownArrow.retain(),
            Key::Left => objc2_ui_kit::UIKeyInputLeftArrow.retain(),
            Key::Right => objc2_ui_kit::UIKeyInputRightArrow.retain(),
            Key::Home => objc2_ui_kit::UIKeyInputHome.retain(),
            Key::End => objc2_ui_kit::UIKeyInputEnd.retain(),
            Key::PageUp => objc2_ui_kit::UIKeyInputPageUp.retain(),
            Key::PageDown => objc2_ui_kit::UIKeyInputPageDown.retain(),
            Key::F(n) => function_key(n)?.retain(),
            // No key of an iPad's or Mac's keyboard.
            Key::Insert => return None,
        }
    };
    let mut flags = UIKeyModifierFlags::empty();
    for (on, flag) in [
        (shortcut.ctrl, UIKeyModifierFlags::Control),
        (shortcut.alt, UIKeyModifierFlags::Alternate),
        (shortcut.shift, UIKeyModifierFlags::Shift),
        (shortcut.meta, UIKeyModifierFlags::Command),
    ] {
        if on {
            flags |= flag;
        }
    }
    Some((input, flags))
}

/// UIKit's input of function key `n` (F1 to F12).
///
/// # Safety
///
/// Reads UIKit's constants.
unsafe fn function_key(n: u8) -> Option<&'static NSString> {
    unsafe extern "C" {
        // Declared twice in UIKit's header, so objc2 leaves it out.
        static UIKeyInputF1: &'static NSString;
    }
    // SAFETY: immutable UIKit constants (iOS 13.4).
    Some(unsafe {
        match n {
            1 => UIKeyInputF1,
            2 => objc2_ui_kit::UIKeyInputF2,
            3 => objc2_ui_kit::UIKeyInputF3,
            4 => objc2_ui_kit::UIKeyInputF4,
            5 => objc2_ui_kit::UIKeyInputF5,
            6 => objc2_ui_kit::UIKeyInputF6,
            7 => objc2_ui_kit::UIKeyInputF7,
            8 => objc2_ui_kit::UIKeyInputF8,
            9 => objc2_ui_kit::UIKeyInputF9,
            10 => objc2_ui_kit::UIKeyInputF10,
            11 => objc2_ui_kit::UIKeyInputF11,
            12 => objc2_ui_kit::UIKeyInputF12,
            _ => return None,
        }
    })
}

/// The entry of command `sender` of the last build, with what it chooses.
fn entry<T>(sender: Option<&AnyObject>, f: impl FnOnce(&Entry) -> T) -> Option<T> {
    let command = sender?.downcast_ref::<UICommand>()?;
    let list = command.propertyList()?;
    let list = list.downcast_ref::<NSNumber>()?.as_u64();
    let (generation, index) = ((list >> 32) as u32, (list & u64::from(u32::MAX)) as usize);
    if generation != GENERATION.with(Cell::get) {
        return None;
    }
    ENTRIES.with(|e| e.borrow().get(index).map(f))
}

/// `canPerformAction:withSender:` for [`action`]: whether command `sender`
/// can be chosen now (UIKit shows the others disabled and passes their
/// keys on).
pub(crate) fn can_choose(mtm: MainThreadMarker, sender: Option<&AnyObject>) -> bool {
    entry(sender, |e| (e.enabled, e.while_editing))
        .is_some_and(|(enabled, while_editing)| enabled && (while_editing || !editing(mtm)))
}

/// [`action`]: reports the item of command `sender` chosen.
pub(crate) fn choose(sender: Option<&AnyObject>) {
    let Some(id) = entry(sender, |e| e.id).filter(|&id| id != 0) else {
        return;
    };
    MENU.with(|m| {
        if let Some(menu) = &*m.borrow() {
            menu.control.dispatch_event(
                ROOT_ID,
                EventKind::Menu,
                EventPayload::Map(vec![("id", PayloadValue::Number(f64::from(id)))]),
            );
        }
    });
}

/// Whether the key window's first responder takes typed text (an input, a
/// web view's text).
fn editing(mtm: MainThreadMarker) -> bool {
    fn first_responder(view: &UIView) -> Option<Retained<UIResponder>> {
        if view.isFirstResponder() {
            return Some(Retained::into_super(view.retain()));
        }
        view.subviews()
            .iter()
            .find_map(|child| first_responder(&child))
    }
    let app = UIApplication::sharedApplication(mtm);
    let window = app
        .connectedScenes()
        .iter()
        .filter_map(|scene| scene.downcast::<UIWindowScene>().ok())
        .flat_map(|scene| scene.windows().to_vec())
        .find(|window| window.isKeyWindow());
    window
        .and_then(|window| first_responder(&window))
        .is_some_and(|responder| crate::keymap::takes_text(&responder))
}
