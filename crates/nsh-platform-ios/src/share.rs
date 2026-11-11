//! The system's share sheet ([`nsh_core::Command::Share`],
//! [`nsh_core::share`]): a `UIActivityViewController` presented over the top
//! controller of the surface's window, so it shows over a sheet or modal
//! already up, once one being dismissed is gone. Where it is a popover
//! (iPad, Mac Catalyst) it points at the anchor's view, or at the middle of
//! the surface without an arrow; UIKit raises an exception for a popover
//! with no source.
//!
//! How it closed goes to the engine in one root `Share` event (`id`,
//! `completed`, `activity`), exactly once: from the controller's completion
//! handler, or, when UIKit lets the controller go without calling it (it
//! never showed, or it went with a controller under it that was
//! dismissed), when the handler is released with it.

use std::{cell::Cell, ptr::NonNull};

use block2::RcBlock;
use dispatch2::{DispatchQueue, MainThreadBound};
use nsh_core::{EventKind, EventPayload, PayloadValue, share::ShareItem};
use nsh_engine::EngineControl;
use objc2::{
    MainThreadMarker, MainThreadOnly, Message,
    rc::Retained,
    runtime::{AnyObject, Bool, ProtocolObject},
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{NSArray, NSError, NSString, NSURL};
use objc2_ui_kit::{
    UIActivityType, UIActivityViewController, UIPopoverArrowDirection, UIView,
    UIViewControllerTransitionCoordinator, UIViewControllerTransitionCoordinatorContext,
};

/// Reports how one share sheet closed, once. The completion handler owns
/// it, and the controller owns the handler: dropped with the controller
/// before the handler ran, it reports the sheet closed without sharing.
struct ShareAnswer {
    id: u32,
    control: EngineControl,
    answered: Cell<bool>,
}

impl ShareAnswer {
    fn answer(&self, completed: bool, activity: Option<String>) {
        if self.answered.replace(true) {
            return;
        }
        let mut fields = vec![
            ("id", PayloadValue::Number(f64::from(self.id))),
            ("completed", PayloadValue::Bool(completed)),
        ];
        if let Some(activity) = activity {
            fields.push(("activity", PayloadValue::Str(activity)));
        }
        self.control.dispatch_event(
            nsh_core::ROOT_ID,
            EventKind::Share,
            EventPayload::Map(fields),
        );
    }
}

impl Drop for ShareAnswer {
    fn drop(&mut self) {
        self.answer(false, None);
    }
}

/// Shows the share sheet with `items` over `root`'s window, a popover
/// pointing at `anchor` (else the middle of `root`), and reports how it
/// closed as request `id`.
pub(crate) fn share(
    id: u32,
    items: &[ShareItem],
    anchor: Option<&UIView>,
    root: &UIView,
    control: &EngineControl,
) {
    let answer = ShareAnswer {
        id,
        control: control.clone(),
        answered: Cell::new(false),
    };
    if root.window().is_none() {
        log::warn!("share: no window to show the share sheet in");
        return answer.answer(false, None);
    }
    let objects: Vec<Retained<AnyObject>> = items.iter().filter_map(activity_item).collect();
    let objects = NSArray::from_retained_slice(&objects);
    // SAFETY: the items are the NSStrings and NSURLs UIKit shares; no
    // activities of the app's own.
    let controller = unsafe {
        UIActivityViewController::initWithActivityItems_applicationActivities(
            root.mtm().alloc(),
            &objects,
            None,
        )
    };
    controller.setOverrideUserInterfaceStyle(crate::system::appearance());
    let completion = RcBlock::new(
        move |activity: *mut UIActivityType, completed: Bool, _: *mut NSArray, _: *mut NSError| {
            // SAFETY: UIKit passes the chosen activity's type or nil.
            let activity = unsafe { activity.as_ref() }.map(NSString::to_string);
            answer.answer(completed.as_bool(), activity);
        },
    );
    // SAFETY: a valid block, which the controller copies (retains) and
    // calls on the main thread with the activity's type or nil.
    unsafe { controller.setCompletionWithItemsHandler(RcBlock::as_ptr(&completion)) };
    present(root.retain(), anchor.map(|view| view.retain()), controller);
}

/// Presents `controller` over the top controller of `root`'s window,
/// pointing its popover (iPad, Mac) at `anchor` while that is in a window,
/// else at the middle of `root`. A controller on the way that is being
/// dismissed (the sheet whose button started the share) is waited for:
/// UIKit refuses to present over a controller dismissing another, and the
/// share sheet would be dropped. Dropped (no window), the sheet answers
/// "not completed" ([`ShareAnswer`]).
fn present(
    root: Retained<UIView>,
    anchor: Option<Retained<UIView>>,
    controller: Retained<UIActivityViewController>,
) {
    let Some(mut top) = root.window().and_then(|window| window.rootViewController()) else {
        return log::warn!("share: no window to show the share sheet in");
    };
    while let Some(next) = top.presentedViewController() {
        if next.isBeingDismissed() {
            if let Some(coordinator) = next.transitionCoordinator() {
                let mtm = root.mtm();
                let waiting = (root.clone(), anchor.clone(), controller.clone());
                let done = RcBlock::new(
                    move |_: NonNull<
                        ProtocolObject<dyn UIViewControllerTransitionCoordinatorContext>,
                    >| {
                        // The coordinator outlives its callbacks: present on
                        // the next main-queue turn.
                        let waiting = MainThreadBound::new(waiting.clone(), mtm);
                        DispatchQueue::main().exec_async(move || {
                            let mtm = MainThreadMarker::new().expect("main queue");
                            let (root, anchor, controller) = waiting.into_inner(mtm);
                            present(root, anchor, controller);
                        });
                    },
                );
                // Refused when the transition is already ending.
                if coordinator.animateAlongsideTransition_completion(None, Some(&done)) {
                    return;
                }
            }
            break;
        }
        top = next;
    }
    if let Some(popover) = controller.popoverPresentationController() {
        match anchor.filter(|view| view.window().is_some()) {
            Some(view) => {
                popover.setSourceView(Some(&view));
                popover.setSourceRect(view.bounds());
            }
            None => {
                let bounds = root.bounds();
                popover.setSourceView(Some(&root));
                popover.setSourceRect(CGRect::new(
                    CGPoint::new(bounds.size.width / 2.0, bounds.size.height / 2.0),
                    CGSize::ZERO,
                ));
                popover.setPermittedArrowDirections(UIPopoverArrowDirection::empty());
            }
        }
    }
    top.presentViewController_animated_completion(&controller, true, None);
}

/// The object UIKit shares for `item`: a string for text, a URL for a link
/// (its text when NSURL takes no such URL), a file URL for a file.
fn activity_item(item: &ShareItem) -> Option<Retained<AnyObject>> {
    let string = |text: &str| Retained::into_super(NSString::from_str(text)).into();
    let url = |url: Retained<NSURL>| Retained::into_super(url).into();
    match item {
        ShareItem::Text(text) => Some(string(text)),
        ShareItem::Url(link) => {
            Some(NSURL::URLWithString(&NSString::from_str(link)).map_or_else(|| string(link), url))
        }
        ShareItem::File(path) => Some(url(NSURL::fileURLWithPath_isDirectory(
            &NSString::from_str(&path.to_string_lossy()),
            false,
        ))),
        // A kind of item a newer core shares: none here.
        _ => None,
    }
}
