//! Views' context menus (`ViewProps::context_menu`): a
//! `UIContextMenuInteraction` on the view, whose delegate gives UIKit a
//! `UIContextMenuConfiguration` that builds the `UIMenu` of the view's
//! current items when the menu opens ([`crate::menu`]). A chosen action
//! reports its id as the root's `Menu` event, as header menus do.
//!
//! The preview is the view itself, lifted: its rounded outline when it has
//! rounded corners, over the background of the nearest view around it that
//! has one ([`crate::mount::Mounter::context_menu_preview`]).
//!
//! The menu takes the long press: once it shows, the presses tracking the
//! touch are cancelled ([`cancel_presses`]: no `onPress` on release, and
//! the pressed style goes), and a long press inside a view with a menu
//! raises no `onLongPress` ([`takes_long_press`]).

use std::{cell::RefCell, ptr::NonNull, rc::Rc};

use block2::RcBlock;
use nsh_core::{EventKind, EventPayload, PayloadValue, desktop::MenuItem};
use nsh_engine::EngineControl;
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject},
};
use objc2_core_foundation::CGPoint;
use objc2_foundation::{NSArray, NSCopying, NSObject, NSObjectProtocol};
use objc2_ui_kit::{
    UIBezierPath, UIColor, UIContextMenuConfiguration, UIContextMenuInteraction,
    UIContextMenuInteractionAnimating, UIContextMenuInteractionDelegate, UIInteraction, UIMenu,
    UIMenuElement, UIPreviewParameters, UITargetedPreview, UIView,
};

use crate::{mount::SharedMounter, press::PressRecognizer};

/// A view's context menu: the interaction on it and its delegate, which
/// UIKit holds weakly. Dropping it takes the interaction off the view.
pub(crate) struct ContextMenu {
    interaction: Retained<UIContextMenuInteraction>,
    delegate: Retained<ContextMenuDelegate>,
}

impl ContextMenu {
    /// Gives `view` the menu of `items` in `slot`: an interaction added for
    /// the first items, its items replaced after, and taken off when there
    /// are none.
    pub(crate) fn sync(
        slot: &mut Option<ContextMenu>,
        view: &UIView,
        items: Vec<MenuItem>,
        control: &EngineControl,
        mounter: &SharedMounter,
        mtm: MainThreadMarker,
    ) {
        if items.is_empty() {
            *slot = None;
            return;
        }
        if let Some(menu) = slot {
            *menu.delegate.ivars().items.borrow_mut() = items;
            return;
        }
        let delegate = ContextMenuDelegate::alloc(mtm).set_ivars(DelegateIvars {
            items: RefCell::new(items),
            control: control.clone(),
            mounter: mounter.clone(),
        });
        // SAFETY: NSObject's designated initializer.
        let delegate: Retained<ContextMenuDelegate> =
            unsafe { objc2::msg_send![super(delegate), init] };
        let interaction = UIContextMenuInteraction::initWithDelegate(
            UIContextMenuInteraction::alloc(mtm),
            ProtocolObject::from_ref(&*delegate),
        );
        view.addInteraction(ProtocolObject::from_ref(&*interaction));
        // Image and text views take no touches by default.
        view.setUserInteractionEnabled(true);
        *slot = Some(ContextMenu {
            interaction,
            delegate,
        });
    }
}

impl Drop for ContextMenu {
    fn drop(&mut self) {
        if let Some(view) = self.interaction.view() {
            view.removeInteraction(ProtocolObject::from_ref(&*self.interaction));
        }
    }
}

pub(crate) struct DelegateIvars {
    /// The items the menu shows, read when it opens.
    items: RefCell<Vec<MenuItem>>,
    control: EngineControl,
    /// For the preview's shape and backdrop, which the mounter knows.
    mounter: SharedMounter,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = DelegateIvars]
    pub(crate) struct ContextMenuDelegate;

    unsafe impl NSObjectProtocol for ContextMenuDelegate {}

    unsafe impl UIContextMenuInteractionDelegate for ContextMenuDelegate {
        #[unsafe(method_id(contextMenuInteraction:configurationForMenuAtLocation:))]
        fn configuration_for_menu(
            &self,
            _interaction: &UIContextMenuInteraction,
            _location: CGPoint,
        ) -> Option<Retained<UIContextMenuConfiguration>> {
            // Taken off the view once it has none; between the two, none.
            let none = self.ivars().items.borrow().is_empty();
            (!none).then(|| self.configuration())
        }

        #[unsafe(method_id(contextMenuInteraction:configuration:highlightPreviewForItemWithIdentifier:))]
        fn highlight_preview(
            &self,
            interaction: &UIContextMenuInteraction,
            _configuration: &UIContextMenuConfiguration,
            _identifier: &ProtocolObject<dyn NSCopying>,
        ) -> Option<Retained<UITargetedPreview>> {
            self.preview(interaction)
        }

        #[unsafe(method_id(contextMenuInteraction:configuration:dismissalPreviewForItemWithIdentifier:))]
        fn dismissal_preview(
            &self,
            interaction: &UIContextMenuInteraction,
            _configuration: &UIContextMenuConfiguration,
            _identifier: &ProtocolObject<dyn NSCopying>,
        ) -> Option<Retained<UITargetedPreview>> {
            self.preview(interaction)
        }

        // iOS 15 asks these; iOS 16 and later the two above.
        #[unsafe(method_id(contextMenuInteraction:previewForHighlightingMenuWithConfiguration:))]
        fn highlight_preview_ios15(
            &self,
            interaction: &UIContextMenuInteraction,
            _configuration: &UIContextMenuConfiguration,
        ) -> Option<Retained<UITargetedPreview>> {
            self.preview(interaction)
        }

        #[unsafe(method_id(contextMenuInteraction:previewForDismissingMenuWithConfiguration:))]
        fn dismissal_preview_ios15(
            &self,
            interaction: &UIContextMenuInteraction,
            _configuration: &UIContextMenuConfiguration,
        ) -> Option<Retained<UITargetedPreview>> {
            self.preview(interaction)
        }

        #[unsafe(method(contextMenuInteraction:willDisplayMenuForConfiguration:animator:))]
        fn will_display_menu(
            &self,
            interaction: &UIContextMenuInteraction,
            _configuration: &UIContextMenuConfiguration,
            _animator: Option<&ProtocolObject<dyn UIContextMenuInteractionAnimating>>,
        ) {
            cancel_presses(interaction);
        }
    }
);

impl ContextMenuDelegate {
    /// A configuration whose menu is made of the items when UIKit shows
    /// it, so items that changed while the finger was down are the ones
    /// shown.
    fn configuration(&self) -> Retained<UIContextMenuConfiguration> {
        let mtm = self.mtm();
        let this = self.retain();
        let provider = RcBlock::new(move |_suggested: NonNull<NSArray<UIMenuElement>>| {
            let items = this.ivars().items.borrow().clone();
            let control = this.ivars().control.clone();
            let select: crate::menu::Select = Rc::new(move |id| choose(&control, id));
            let menu = crate::menu::make_menu(mtm, &items, &select);
            Retained::autorelease_return(menu)
        });
        // SAFETY: UIKit copies the provider and calls it on the main thread;
        // it returns an autoreleased menu, as a block returning an object
        // does. No preview provider: the view itself is the preview.
        unsafe {
            UIContextMenuConfiguration::configurationWithIdentifier_previewProvider_actionProvider(
                None,
                std::ptr::null_mut(),
                (&*provider as *const block2::DynBlock<dyn Fn(_) -> *mut UIMenu>).cast_mut(),
                mtm,
            )
        }
    }

    /// The view lifted as it is: its rounded outline, over the backdrop
    /// around it. `None` (UIKit's own) once the view left its window, as a
    /// row removed while its menu was open.
    fn preview(
        &self,
        interaction: &UIContextMenuInteraction,
    ) -> Option<Retained<UITargetedPreview>> {
        let mtm = self.mtm();
        let view = interaction.view()?;
        view.window()?;
        let parameters = UIPreviewParameters::new(mtm);
        let shape = match self.ivars().mounter.get(mtm).try_borrow() {
            Ok(m) => m.as_ref().map(|m| m.context_menu_preview(&view)),
            Err(_) => None,
        };
        if let Some((path, background)) = shape {
            if let Some(path) = path {
                parameters.setVisiblePath(Some(&UIBezierPath::bezierPathWithCGPath(&path)));
            }
            if let Some(color) = background.and_then(crate::decor::cg_color) {
                parameters.setBackgroundColor(Some(&UIColor::colorWithCGColor(&color)));
            }
        }
        Some(UITargetedPreview::initWithView_parameters(
            UITargetedPreview::alloc(mtm),
            &view,
            &parameters,
        ))
    }
}

/// Reports a chosen action as the root's `Menu` event.
fn choose(control: &EngineControl, id: u32) {
    control.dispatch_event(
        nsh_core::ROOT_ID,
        EventKind::Menu,
        EventPayload::Map(vec![("id", PayloadValue::Number(f64::from(id)))]),
    );
}

/// Cancels the presses tracking the touch that opened the menu: those of
/// the view under it and of every view around that, up to the window.
/// UIKit hands the touch to the menu, and a press's release must not
/// follow (nor its pressed style stay). Turning a recognizer off and on
/// cancels it; its reset reports the press out.
fn cancel_presses(interaction: &UIContextMenuInteraction) {
    let Some(view) = interaction.view() else {
        return;
    };
    let at = interaction.locationInView(Some(&view));
    let mut current = view.hitTest_withEvent(at, None).or(Some(view));
    while let Some(v) = current {
        if let Some(recognizers) = v.gestureRecognizers() {
            for r in recognizers.iter() {
                if r.downcast_ref::<PressRecognizer>().is_some() && r.isEnabled() {
                    r.setEnabled(false);
                    r.setEnabled(true);
                }
            }
        }
        current = v.superview();
    }
}

/// Whether a long press on `view` opens a context menu: it, or a view
/// around it, has one. Its presses then raise no `onLongPress`.
pub(crate) fn takes_long_press(view: &UIView) -> bool {
    let mut current = Some(view.retain());
    while let Some(v) = current {
        if v.interactions().iter().any(|i| is_context_menu(&i)) {
            return true;
        }
        current = v.superview();
    }
    false
}

/// Whether `interaction` is a view's context menu of NSH's.
fn is_context_menu(interaction: &ProtocolObject<dyn UIInteraction>) -> bool {
    let object: &AnyObject = interaction.as_ref();
    object
        .downcast_ref::<UIContextMenuInteraction>()
        .and_then(|i| i.delegate())
        .is_some_and(|d| {
            let delegate: &AnyObject = d.as_ref();
            delegate.downcast_ref::<ContextMenuDelegate>().is_some()
        })
}
