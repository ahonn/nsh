//! Swipe actions (`ViewProps::swipe_actions`, [`nsh_core::swipe`]): buttons
//! a sideways swipe reveals at a view's leading or trailing edge, as Mail's
//! rows show them. NSH's lists are scroll views of its own views, not table
//! views, so `UISwipeActionsConfiguration` is not there: any `NshView` gets
//! a [`SwipeRow`] with a pan and a tray of buttons instead.
//!
//! - The swipe moves the view by the last step of its layer's transform
//!   (`mount.rs` `dynamic_transform` appends [`NshView::swipe_offset`] to
//!   the style's), so its frame and the app's transforms stay as the core
//!   set them; a restyle mid-swipe keeps the offset. While the finger
//!   drags it, a style transition of its transform is dropped, so it
//!   follows the finger; one during the settling spring takes the spring's
//!   place and still ends where the spring would. A sticky list cell's
//!   pinning sets its transform outright: no swipe actions on those.
//! - The buttons are in a tray, a subview of the view laid just outside
//!   its bounds on the edge revealed, as wide as the move reveals; the
//!   view's hit test reaches it there (`NshView::hit`). A view that clips
//!   its content (`overflow: hidden`) clips the tray too: give swipe actions
//!   to a view that does not clip, and clip inside it.
//! - The pan starts only for a sideways move toward an edge with actions
//!   ([`nsh_core::swipe::may_start`]), and the pans of the scroll views
//!   around wait for it to fail: a vertical drag scrolls, a swipe never
//!   does. It follows the finger 1:1 up to the buttons' width, then
//!   resists; on release it settles open past half of it (a flick counts),
//!   else closed, with a spring (a short ease with Reduce Motion). A full
//!   swipe only reveals: no action runs without a press on its button.
//! - Going back wins at the screen's edge: the pan waits for a navigation
//!   controller's edge pop to fail, and a move toward going back that
//!   starts within [`BACK_EDGE`] of the leading edge of the window does not
//!   start it while the stack can go back. iOS 26's content pop (a swipe
//!   back from anywhere on the page) waits for the pan to fail instead, so
//!   away from the edge a view with leading actions takes the swipe right,
//!   and one without leaves it to going back.
//! - One view of a surface is open at a time ([`SwipeGroup`], held by the
//!   surface's `EventTarget`): starting to swipe another closes it; a
//!   scroll closes it; a press on another view in its scroll view closes it
//!   and is no press; a tap on it closes it and presses nothing in it. A
//!   button closes it, then reports its id as the root's `Menu` event.
//! - Recycled list rows keep their view: new actions that look the same
//!   on the open edge keep it open (only the ids change); others close it.
//!   A view leaving its window closes at once; removing the actions takes
//!   the gesture, the tray and the offset away.
//! - A context menu on the view (`context_menu.rs`) keeps its long press:
//!   its `UIContextMenuInteraction` recognizes once the finger has rested
//!   a moment, before the menu shows, which would prevent the pan, so the
//!   pan recognizes alongside it and, when it starts, cancels it (a
//!   sideways move before the menu shows swipes, as in Mail). Once the
//!   menu shows, the pan is cancelled for the rest of the touch
//!   ([`context_menu_shown`]): a drag on the preview never swipes.
//! - Each button is its own color: the system's red for a destructive one,
//!   the app's tint, else blue at the leading edge and gray at the trailing
//!   edge; a button the color of the one outside it (two destructive ones)
//!   is a shade darker, so the two read as two buttons.
//! - VoiceOver offers the actions as the view's custom actions
//!   (`native_view.rs`); the tray is hidden from it.

use std::cell::{Cell, RefCell};

use block2::RcBlock;
use nsh_core::{
    EventKind, EventPayload, Haptic, HapticImpact, PayloadValue, Points, ROOT_ID,
    geometry::point,
    swipe::{
        MIN_BUTTON_WIDTH, SwipeActions, SwipeButton, SwipeEdge, button_frames, may_start,
        rubber_band, settles_open,
    },
};
use nsh_engine::EngineControl;
use objc2::{
    ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send,
    rc::{Retained, Weak},
    runtime::{AnyClass, AnyObject, Bool, NSObject, NSObjectProtocol},
    sel,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_foundation::NSString;
use objc2_quartz_core::{CALayer, CATransform3D};
use objc2_ui_kit::{
    NSObjectUIAccessibility, NSTextAlignment, UIColor, UIControl, UIControlEvents, UIEvent, UIFont,
    UIFontWeightMedium, UIFontWeightSemibold, UIGestureRecognizer, UIGestureRecognizerDelegate,
    UIGestureRecognizerState, UIImage, UILabel, UIPanGestureRecognizer, UIResponder, UIScrollView,
    UITapGestureRecognizer, UITouch, UIUserInterfaceLayoutDirection, UIView,
    UIViewAnimationOptions, UIViewAutoresizing, UIViewController,
};

use crate::{decor::quiet, native_view::NshView, target::EventTarget};

/// How close to the leading edge of the window a move toward going back
/// must start to go back rather than swipe a view, while the stack can go
/// back (about the width of UIKit's edge pan).
pub(crate) const BACK_EDGE: CGFloat = 24.0;

/// How far ahead a release's speed is projected, in seconds: a flick
/// opens or closes a view that has not moved half way yet.
const LOOK_AHEAD: CGFloat = 0.1;

/// The tag of the tray's views: no node's (NSH reads a view's tag as its
/// node id).
const NO_NODE: isize = -1;

/// Space on either side of a button's label.
const PADDING: CGFloat = 16.0;

/// Between a button's symbol and its label.
const ICON_GAP: CGFloat = 4.0;

/// The point size of a button's symbol.
const ICON_SIZE: CGFloat = 20.0;

/// How much darker (black over it) a button is than the one beside it of
/// the same color.
const SHADE: CGFloat = 0.22;

/// One button in the tray.
struct Button {
    control: Retained<UIControl>,
    /// Holds the symbol and the label, as wide as the button's own width,
    /// at the button's inner side: a button narrower than that (half
    /// revealed) shows its inner part, as if it slid out from under the
    /// view.
    content: Retained<UIView>,
    /// The symbol, if it has one, over the label.
    icon: Option<Retained<UIView>>,
    label: Retained<UILabel>,
    /// Darkens the button while it is pressed.
    dim: Retained<UIView>,
    /// Its own width: the label's, with padding, at least
    /// [`MIN_BUTTON_WIDTH`].
    width: CGFloat,
}

/// The view open (or opening) in a surface, and the touch a press on
/// another view gave up to close it.
#[derive(Default)]
pub(crate) struct SwipeGroup {
    open: RefCell<Option<Weak<SwipeRow>>>,
    /// Every pressable along a touch asks; the first one's answer stands.
    taken: RefCell<Option<Weak<UITouch>>>,
}

impl SwipeGroup {
    fn current(&self) -> Option<Retained<SwipeRow>> {
        self.open.borrow().as_ref().and_then(Weak::load)
    }

    /// `row` opens: another one open closes.
    fn opening(&self, row: &SwipeRow) {
        if let Some(other) = self.current()
            && !std::ptr::eq::<SwipeRow>(&*other, row)
        {
            other.close(true);
        }
        *self.open.borrow_mut() = Some(Weak::new(row));
    }

    /// `row` closed.
    fn closed(&self, row: &SwipeRow) {
        let mut open = self.open.borrow_mut();
        if open
            .as_ref()
            .and_then(Weak::load)
            .is_none_or(|r| std::ptr::eq::<SwipeRow>(&*r, row))
        {
            *open = None;
        }
    }

    /// A scroll view started to scroll: the open view closes.
    pub(crate) fn close(&self) {
        if let Some(row) = self.current() {
            row.close(true);
        }
    }

    /// Whether the press `touch` starts is given up for the open view (the
    /// press recognizer fails): a touch on the open view, which its tap
    /// closes, or on another view in its scroll view, which closes it now.
    /// A touch on its buttons is theirs; a touch outside its scroll view
    /// closes it and presses as usual.
    pub(crate) fn takes_press(&self, touch: &UITouch) -> bool {
        if self
            .taken
            .borrow()
            .as_ref()
            .and_then(Weak::load)
            .is_some_and(|t| std::ptr::eq::<UITouch>(&*t, touch))
        {
            return true;
        }
        let Some(row) = self.current() else {
            return false;
        };
        let (Some(view), Some(touched)) = (row.view(), touch.view()) else {
            return false;
        };
        if touched.isDescendantOfView(&row.ivars().tray) {
            return false;
        }
        let take = if touched.isDescendantOfView(&view) {
            true
        } else {
            row.close(true);
            same_scroll_view(&touched, &view)
        };
        if take {
            *self.taken.borrow_mut() = Some(Weak::new(touch));
        }
        take
    }
}

pub(crate) struct RowIvars {
    view: Weak<NshView>,
    /// The surface's target: its [`SwipeGroup`].
    target: Retained<EventTarget>,
    control: RefCell<EngineControl>,
    actions: RefCell<SwipeActions>,
    pan: RefCell<Option<Retained<UIPanGestureRecognizer>>>,
    /// Closes the open view on a tap (enabled while it is open).
    tap: RefCell<Option<Retained<UITapGestureRecognizer>>>,
    tray: Retained<UIView>,
    buttons: RefCell<Vec<Button>>,
    /// The edge whose buttons are in the tray.
    tray_edge: Cell<Option<SwipeEdge>>,
    /// The tray is on the view's left (the view moved right): where it
    /// stays while it closes.
    tray_left: Cell<bool>,
    /// The view's offset (positive to the right), the last step of its
    /// transform.
    offset: Cell<CGFloat>,
    /// The offset when the pan began.
    start: Cell<CGFloat>,
    /// The edge the view rests open on.
    open: Cell<Option<SwipeEdge>>,
    /// The view is past half of its buttons (a light tap marks crossing).
    past_half: Cell<bool>,
}

define_class!(
    /// The swipe gesture, tray and state of one view.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = RowIvars]
    pub(crate) struct SwipeRow;

    unsafe impl NSObjectProtocol for SwipeRow {}

    impl SwipeRow {
        #[unsafe(method(handlePan:))]
        fn handle_pan(&self, pan: &UIPanGestureRecognizer) {
            self.pan_changed(pan);
        }

        #[unsafe(method(handleTap:))]
        fn handle_tap(&self, tap: &UITapGestureRecognizer) {
            if tap.state() == UIGestureRecognizerState::Ended {
                self.close(true);
            }
        }

        #[unsafe(method(buttonDown:))]
        fn button_down(&self, sender: &UIControl) {
            self.dim(sender, true);
        }

        #[unsafe(method(buttonUp:))]
        fn button_up(&self, sender: &UIControl) {
            self.dim(sender, false);
        }

        #[unsafe(method(buttonPressed:))]
        fn button_pressed(&self, sender: &UIControl) {
            self.dim(sender, false);
            let index = self
                .ivars()
                .buttons
                .borrow()
                .iter()
                .position(|b| std::ptr::eq::<UIControl>(&*b.control, sender));
            let id = self.ivars().tray_edge.get().zip(index).and_then(|(edge, i)| {
                let actions = self.ivars().actions.borrow();
                actions.edge(edge).get(i).map(|b| b.id)
            });
            self.close(true);
            if let Some(id) = id {
                report(&self.ivars().control.borrow(), id);
            }
        }
    }

    unsafe impl UIGestureRecognizerDelegate for SwipeRow {
        #[unsafe(method(gestureRecognizerShouldBegin:))]
        fn should_begin(&self, recognizer: &UIGestureRecognizer) -> bool {
            match recognizer.downcast_ref::<UIPanGestureRecognizer>() {
                Some(pan) => self.pan_should_begin(pan),
                None => self.ivars().open.get().is_some(),
            }
        }

        /// The tap that closes the view leaves its buttons to them.
        #[unsafe(method(gestureRecognizer:shouldReceiveTouch:))]
        fn should_receive_touch(&self, recognizer: &UIGestureRecognizer, touch: &UITouch) -> bool {
            recognizer.isKindOfClass(UIPanGestureRecognizer::class())
                || !touch
                    .view()
                    .is_some_and(|v| v.isDescendantOfView(&self.ivars().tray))
        }

        /// A context menu on the view does not hold the swipe back once its
        /// long press started (its recognizers recognize before the menu
        /// shows, which would prevent the pan): a sideways move before the
        /// menu shows swipes, and cancels the menu ([`cancel_context_menu`]).
        #[unsafe(method(gestureRecognizer:shouldRecognizeSimultaneouslyWithGestureRecognizer:))]
        fn should_recognize_simultaneously(
            &self,
            recognizer: &UIGestureRecognizer,
            other: &UIGestureRecognizer,
        ) -> bool {
            recognizer.isKindOfClass(UIPanGestureRecognizer::class())
                && self.view().is_some_and(|v| is_context_menu_recognizer(other, &v))
        }

        /// The edge pop of a navigation controller goes first.
        #[unsafe(method(gestureRecognizer:shouldRequireFailureOfGestureRecognizer:))]
        fn should_require_failure_of(
            &self,
            recognizer: &UIGestureRecognizer,
            other: &UIGestureRecognizer,
        ) -> bool {
            recognizer.isKindOfClass(UIPanGestureRecognizer::class())
                && self.view().is_some_and(|v| pop_of(other, &v) == Some(Pop::Edge))
        }

        /// The scroll views around and iOS 26's content pop wait for the
        /// swipe to fail.
        #[unsafe(method(gestureRecognizer:shouldBeRequiredToFailByGestureRecognizer:))]
        fn should_be_required_to_fail_by(
            &self,
            recognizer: &UIGestureRecognizer,
            other: &UIGestureRecognizer,
        ) -> bool {
            recognizer.isKindOfClass(UIPanGestureRecognizer::class())
                && self.view().is_some_and(|v| {
                    pop_of(other, &v) == Some(Pop::Content) || is_scroll_pan_around(other, &v)
                })
        }
    }
);

/// Gives `view` the swipe actions `actions` (none: takes them away).
pub(crate) fn sync(
    view: &NshView,
    actions: &SwipeActions,
    target: &Retained<EventTarget>,
    control: &EngineControl,
) {
    match (view.swipe(), actions.is_empty()) {
        (None, true) => {}
        (Some(row), true) => {
            row.detach();
            view.set_swipe(None);
        }
        (Some(row), false) => {
            *row.ivars().control.borrow_mut() = control.clone();
            row.set_actions(actions);
        }
        (None, false) => {
            let row = SwipeRow::attach(view, actions.clone(), target, control);
            view.set_swipe(Some(row));
        }
    }
}

impl SwipeRow {
    fn attach(
        view: &NshView,
        actions: SwipeActions,
        target: &Retained<EventTarget>,
        control: &EngineControl,
    ) -> Retained<Self> {
        let mtm = view.mtm();
        let tray = UIView::new(mtm);
        tray.setTag(NO_NODE);
        tray.setClipsToBounds(true);
        tray.setHidden(true);
        tray.setAccessibilityElementsHidden(true, mtm);
        view.insertSubview_atIndex(&tray, 0);
        let this = Self::alloc(mtm).set_ivars(RowIvars {
            view: Weak::new(view),
            target: target.clone(),
            control: RefCell::new(control.clone()),
            actions: RefCell::new(actions),
            pan: RefCell::new(None),
            tap: RefCell::new(None),
            tray,
            buttons: RefCell::new(Vec::new()),
            tray_edge: Cell::new(None),
            tray_left: Cell::new(false),
            offset: Cell::new(0.0),
            start: Cell::new(0.0),
            open: Cell::new(None),
            past_half: Cell::new(false),
        });
        // SAFETY: NSObject's designated initializer.
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        let target_object: &AnyObject = &this;
        let delegate = objc2::runtime::ProtocolObject::from_ref(&*this);
        // SAFETY: the actions are implemented by the row, which the view
        // holds as long as the recognizers are attached to it; so is the
        // delegate.
        let (pan, tap) = unsafe {
            let pan = UIPanGestureRecognizer::initWithTarget_action(
                UIPanGestureRecognizer::alloc(mtm),
                Some(target_object),
                Some(sel!(handlePan:)),
            );
            let tap = UITapGestureRecognizer::initWithTarget_action(
                UITapGestureRecognizer::alloc(mtm),
                Some(target_object),
                Some(sel!(handleTap:)),
            );
            pan.setDelegate(Some(delegate));
            tap.setDelegate(Some(delegate));
            (pan, tap)
        };
        pan.setMaximumNumberOfTouches(1);
        tap.setEnabled(false);
        view.addGestureRecognizer(&pan);
        view.addGestureRecognizer(&tap);
        *this.ivars().pan.borrow_mut() = Some(pan);
        *this.ivars().tap.borrow_mut() = Some(tap);
        this
    }

    fn view(&self) -> Option<Retained<NshView>> {
        self.ivars().view.load()
    }

    /// The view's offset (positive to the right).
    pub(crate) fn offset(&self) -> CGFloat {
        self.ivars().offset.get()
    }

    /// The button at `point` (in `view`, the view swiped), while the
    /// buttons show: they lie outside its bounds, where UIKit's hit test
    /// does not look.
    pub(crate) fn hit_buttons(
        &self,
        view: &UIView,
        point: CGPoint,
        event: Option<&UIEvent>,
    ) -> Option<Retained<UIView>> {
        let tray = &self.ivars().tray;
        if tray.isHidden()
            || view.isHidden()
            || !view.isUserInteractionEnabled()
            || view.alpha() <= 0.01
        {
            return None;
        }
        let local = view.convertPoint_toView(point, Some(tray));
        if !tray.pointInside_withEvent(local, event) {
            return None;
        }
        tray.hitTest_withEvent(local, event)
    }

    /// Takes the gesture, the tray and the offset off the view.
    fn detach(&self) {
        self.reset();
        if let Some(view) = self.view() {
            for recognizer in [
                self.ivars().pan.take().map(Retained::into_super),
                self.ivars().tap.take().map(Retained::into_super),
            ]
            .into_iter()
            .flatten()
            {
                view.removeGestureRecognizer(&recognizer);
            }
        }
        self.clear_buttons();
        self.ivars().tray.removeFromSuperview();
    }

    /// New actions: the view stays open if the edge it shows looks the
    /// same (a recycled row bound to another item gets new ids), else it
    /// closes and the tray is made again when it is next revealed.
    fn set_actions(&self, actions: &SwipeActions) {
        let old = self.ivars().actions.replace(actions.clone());
        let Some(edge) = self.ivars().tray_edge.get() else {
            return;
        };
        let (was, now) = (old.edge(edge), actions.edge(edge));
        let same = was.len() == now.len() && was.iter().zip(now).all(|(a, b)| a.looks_like(b));
        if !same {
            self.reset();
            self.clear_buttons();
        }
    }

    /// The view's direction, as UIKit lays it out (`mount.rs` `mirror`).
    fn rtl(&self, view: &UIView) -> bool {
        view.effectiveUserInterfaceLayoutDirection() == UIUserInterfaceLayoutDirection::RightToLeft
    }

    fn pan_should_begin(&self, pan: &UIPanGestureRecognizer) -> bool {
        let Some(view) = self.view() else {
            return false;
        };
        let velocity = pan.velocityInView(None);
        let translation = pan.translationInView(None);
        // The direction so far; the velocity says it best once moving.
        let motion = if velocity.x == 0.0 && velocity.y == 0.0 {
            translation
        } else {
            velocity
        };
        let rtl = self.rtl(&view);
        // Mid-animation counts as open: the finger catches the view.
        let open = self
            .ivars()
            .open
            .get()
            .or_else(|| SwipeEdge::revealed_by(Points(self.offset() as f32), rtl));
        let actions = self.ivars().actions.borrow();
        if !may_start(
            point(Points(motion.x as f32), Points(motion.y as f32)),
            &actions,
            open,
            rtl,
        ) {
            return false;
        }
        open.is_some() || !yields_to_back(pan, &view, motion.x > 0.0, rtl)
    }

    fn pan_changed(&self, pan: &UIPanGestureRecognizer) {
        let Some(view) = self.view() else { return };
        let ivars = self.ivars();
        let rtl = self.rtl(&view);
        match pan.state() {
            UIGestureRecognizerState::Began => {
                let offset = self.catch(&view);
                ivars.start.set(offset);
                ivars.open.set(None);
                ivars.past_half.set(false);
                if let Some(tap) = ivars.tap.borrow().as_ref() {
                    tap.setEnabled(false);
                }
                ivars.target.swipes().opening(self);
                cancel_context_menu(&view);
            }
            UIGestureRecognizerState::Changed => {
                // In the window: the view moves under the finger.
                let raw = ivars.start.get() + pan.translationInView(None).x;
                let edge = SwipeEdge::revealed_by(Points(raw as f32), rtl);
                let width = edge.map_or(0.0, |e| self.reveal(&view, e));
                let offset = rubber_band(
                    Points(raw as f32),
                    Points(width as f32),
                    Points(view.bounds().size.width as f32),
                )
                .0 as CGFloat;
                let past = width > 0.0 && offset.abs() > width / 2.0;
                if past && !ivars.past_half.get() {
                    crate::system::haptic(Haptic::Impact(HapticImpact::Light));
                }
                ivars.past_half.set(past);
                // The view follows the finger in the same frame: a style
                // transition of its transform (a pressed style the swipe
                // released) would hold it at the transition's end value.
                view.layer()
                    .removeAnimationForKey(objc2_foundation::ns_string!("transform"));
                quiet(|| {
                    self.move_to(&view, offset);
                });
                self.hide_tray_if_closed();
            }
            UIGestureRecognizerState::Ended | UIGestureRecognizerState::Cancelled => {
                let velocity = pan.velocityInView(None).x;
                let offset = self.offset();
                let edge = SwipeEdge::revealed_by(Points(offset as f32), rtl);
                let width = edge.map_or(0.0, |e| self.edge_width(e));
                let projected = offset + velocity * LOOK_AHEAD;
                let opens = pan.state() == UIGestureRecognizerState::Ended
                    && edge.is_some()
                    && SwipeEdge::revealed_by(Points(projected as f32), rtl) == edge
                    && settles_open(Points(projected as f32), Points(width as f32));
                match edge.filter(|_| opens) {
                    Some(edge) => {
                        let to = if edge.moves_right(rtl) { width } else { -width };
                        ivars.open.set(Some(edge));
                        // Another view swiped meanwhile (a second finger)
                        // closes: this one is the open one.
                        ivars.target.swipes().opening(self);
                        if let Some(tap) = ivars.tap.borrow().as_ref() {
                            tap.setEnabled(true);
                        }
                        self.animate_to(&view, to, velocity);
                    }
                    None => {
                        ivars.target.swipes().closed(self);
                        self.animate_to(&view, 0.0, velocity);
                    }
                }
            }
            _ => {}
        }
    }

    /// Closes the view: along a spring, or at once.
    fn close(&self, animated: bool) {
        let ivars = self.ivars();
        ivars.open.set(None);
        ivars.past_half.set(false);
        if let Some(tap) = ivars.tap.borrow().as_ref() {
            tap.setEnabled(false);
        }
        ivars.target.swipes().closed(self);
        let Some(view) = self.view() else { return };
        if animated && self.offset() != 0.0 {
            self.animate_to(&view, 0.0, 0.0);
        } else {
            self.reset();
        }
    }

    /// Closes the view at once, mid-animation too: it left its window, or
    /// its actions changed.
    pub(crate) fn reset(&self) {
        let ivars = self.ivars();
        ivars.open.set(None);
        ivars.past_half.set(false);
        if let Some(tap) = ivars.tap.borrow().as_ref() {
            tap.setEnabled(false);
        }
        ivars.target.swipes().closed(self);
        if let Some(view) = self.view() {
            self.stop_animations(&view);
            quiet(|| self.move_to(&view, 0.0));
        }
        self.hide_tray_if_closed();
    }

    /// Hides the tray of a closed view (from the screen reader and hit
    /// tests too); not in an animation that closes it, which would hide it
    /// at its start.
    fn hide_tray_if_closed(&self) {
        if self.offset() == 0.0 {
            self.ivars().tray.setHidden(true);
        }
    }

    /// Stops a running animation where it is; the offset shown.
    fn catch(&self, view: &NshView) -> CGFloat {
        let layer = view.layer();
        let model = self.offset();
        // SAFETY: the presentation layer is only read.
        let shown = match unsafe { layer.presentationLayer() } {
            Some(presented) if layer.animationKeys().is_some_and(|k| k.count() > 0) => {
                model + (presented.transform().m41 - layer.transform().m41)
            }
            _ => model,
        };
        self.stop_animations(view);
        quiet(|| self.move_to(view, shown));
        shown
    }

    fn stop_animations(&self, view: &NshView) {
        // The key UIView animations of the transform use.
        view.layer()
            .removeAnimationForKey(objc2_foundation::ns_string!("transform"));
        let tray = &self.ivars().tray;
        remove_animations(tray);
    }

    /// Moves the view to `offset` and lays the tray out for it, in the
    /// current animation if there is one.
    fn move_to(&self, view: &NshView, offset: CGFloat) {
        let old = self.ivars().offset.replace(offset);
        if old != offset {
            // The offset is the transform's last step (`dynamic_transform`):
            // moving it is a translation after what is there.
            let layer = view.layer();
            let moved =
                layer
                    .transform()
                    .concat(CATransform3D::new_translation(offset - old, 0.0, 0.0));
            layer.setTransform(moved);
        }
        self.lay_out_tray(view, offset);
    }

    /// The tray beside the view on the edge `offset` reveals, as wide as
    /// it reveals; its buttons share that width from the outer edge.
    fn lay_out_tray(&self, view: &NshView, offset: CGFloat) {
        let tray = &self.ivars().tray;
        let bounds = view.bounds();
        let revealed = offset.abs();
        // Closed, it stays on the side it closed on (an animation to zero
        // must not move it across the view).
        if offset != 0.0 {
            self.ivars().tray_left.set(offset > 0.0);
        }
        let moved_right = self.ivars().tray_left.get();
        let x = if moved_right {
            bounds.origin.x - revealed
        } else {
            bounds.origin.x + bounds.size.width
        };
        tray.setFrame(CGRect::new(
            CGPoint::new(x, bounds.origin.y),
            CGSize::new(revealed, bounds.size.height),
        ));
        // The tray keeps to the view's edge if the view's size changes.
        tray.setAutoresizingMask(
            UIViewAutoresizing::FlexibleHeight
                | if moved_right {
                    UIViewAutoresizing::FlexibleRightMargin
                } else {
                    UIViewAutoresizing::FlexibleLeftMargin
                },
        );
        if revealed > 0.0 {
            tray.setHidden(false);
        }
        let buttons = self.ivars().buttons.borrow();
        let widths: Vec<Points> = buttons.iter().map(|b| Points(b.width as f32)).collect();
        let frames = button_frames(&widths, Points(revealed as f32), moved_right);
        let height = bounds.size.height;
        for (button, (x, width)) in buttons.iter().zip(frames) {
            let (x, width) = (x.0 as CGFloat, width.0 as CGFloat);
            button.control.setFrame(CGRect::new(
                CGPoint::new(x, 0.0),
                CGSize::new(width, height),
            ));
            button.dim.setFrame(button.control.bounds());
            // The inner side is the one toward the view.
            let content_x = if moved_right {
                width - button.width
            } else {
                0.0
            };
            button.content.setFrame(CGRect::new(
                CGPoint::new(content_x, 0.0),
                CGSize::new(button.width, height),
            ));
            button.lay_out_content(height);
        }
    }

    /// Puts the buttons of `edge` in the tray (if they are not there) and
    /// returns their total width.
    fn reveal(&self, view: &NshView, edge: SwipeEdge) -> CGFloat {
        if self.ivars().tray_edge.get() != Some(edge) {
            self.clear_buttons();
            let actions = self.ivars().actions.borrow().clone();
            let mtm = view.mtm();
            let made: Vec<Button> = actions
                .edge(edge)
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    let outer = i.checked_sub(1).map(|o| &actions.edge(edge)[o]);
                    self.make_button(b, outer, edge, mtm)
                })
                .collect();
            for button in &made {
                self.ivars().tray.addSubview(&button.control);
            }
            *self.ivars().buttons.borrow_mut() = made;
            self.ivars().tray_edge.set(Some(edge));
        }
        self.edge_width(edge)
    }

    /// The total width of `edge`'s buttons, once they are in the tray.
    fn edge_width(&self, edge: SwipeEdge) -> CGFloat {
        if self.ivars().tray_edge.get() != Some(edge) {
            return 0.0;
        }
        self.ivars().buttons.borrow().iter().map(|b| b.width).sum()
    }

    fn clear_buttons(&self) {
        for button in self.ivars().buttons.take() {
            // A finger still on it must not reach the row once it is gone:
            // controls do not retain their targets.
            // SAFETY: removing every target-action pair of the control.
            unsafe {
                button.control.removeTarget_action_forControlEvents(
                    None,
                    None,
                    UIControlEvents::AllEvents,
                )
            };
            button.control.removeFromSuperview();
        }
        self.ivars().tray_edge.set(None);
    }

    /// A button for `button`; `outer` is the one beside it toward the
    /// outer edge, if any.
    fn make_button(
        &self,
        button: &SwipeButton,
        outer: Option<&SwipeButton>,
        edge: SwipeEdge,
        mtm: MainThreadMarker,
    ) -> Button {
        let control = UIControl::new(mtm);
        control.setTag(NO_NODE);
        control.setClipsToBounds(true);
        control.setBackgroundColor(Some(&background(button, edge)));
        // A button the color of the one beside it (two destructive ones)
        // is a shade darker, so each shows as a button of its own.
        if outer.is_some_and(|o| same_fill(o, button)) {
            let shade = UIView::new(mtm);
            shade.setTag(NO_NODE);
            shade.setUserInteractionEnabled(false);
            shade.setBackgroundColor(Some(&UIColor::colorWithWhite_alpha(0.0, SHADE)));
            shade.setAutoresizingMask(
                UIViewAutoresizing::FlexibleWidth | UIViewAutoresizing::FlexibleHeight,
            );
            // Sized with the button: it is made zero-sized, and grows with it.
            control.addSubview(&shade);
        }
        let content = UIView::new(mtm);
        content.setTag(NO_NODE);
        content.setUserInteractionEnabled(false);
        let dim = UIView::new(mtm);
        dim.setTag(NO_NODE);
        dim.setUserInteractionEnabled(false);
        dim.setHidden(true);
        dim.setBackgroundColor(Some(&UIColor::colorWithWhite_alpha(0.0, 0.18)));
        control.addSubview(&dim);
        control.addSubview(&content);

        let icon = button
            .symbol
            .as_deref()
            .and_then(|name| symbol_view(name, mtm));
        let label = UILabel::new(mtm);
        label.setTag(NO_NODE);
        label.setText(Some(&NSString::from_str(&button.label)));
        label.setTextAlignment(NSTextAlignment::Center);
        label.setNumberOfLines(1);
        // SAFETY: a system font and color.
        unsafe {
            let (size, weight) = if icon.is_some() {
                (13.0, UIFontWeightMedium)
            } else {
                (15.0, UIFontWeightSemibold)
            };
            label.setFont(Some(&UIFont::systemFontOfSize_weight(size, weight)));
            label.setTextColor(Some(&UIColor::whiteColor()));
        }
        label.sizeToFit();
        let label_size = label.bounds().size;
        let icon_size = icon
            .as_ref()
            .map_or(CGSize::new(0.0, 0.0), |i| i.bounds().size);
        let width = (label_size.width.max(icon_size.width) + 2.0 * PADDING)
            .max(MIN_BUTTON_WIDTH.0 as CGFloat)
            .ceil();
        if let Some(icon) = &icon {
            content.addSubview(icon);
        }
        content.addSubview(&label);

        let target: &AnyObject = self;
        // SAFETY: the actions are implemented by the row, which holds the
        // buttons.
        unsafe {
            control.addTarget_action_forControlEvents(
                Some(target),
                sel!(buttonDown:),
                UIControlEvents::TouchDown | UIControlEvents::TouchDragEnter,
            );
            control.addTarget_action_forControlEvents(
                Some(target),
                sel!(buttonUp:),
                UIControlEvents::TouchDragExit
                    | UIControlEvents::TouchUpOutside
                    | UIControlEvents::TouchCancel,
            );
            control.addTarget_action_forControlEvents(
                Some(target),
                sel!(buttonPressed:),
                UIControlEvents::TouchUpInside,
            );
        }
        Button {
            control,
            content,
            icon,
            label,
            dim,
            width,
        }
    }

    fn dim(&self, control: &UIControl, on: bool) {
        if let Some(button) = self
            .ivars()
            .buttons
            .borrow()
            .iter()
            .find(|b| std::ptr::eq::<UIControl>(&*b.control, control))
        {
            button.dim.setHidden(!on);
        }
    }

    /// Moves the view to `to` along a spring that starts at the finger's
    /// `velocity` (points per second), or a short ease with Reduce Motion.
    fn animate_to(&self, view: &NshView, to: CGFloat, velocity: CGFloat) {
        let distance = to - self.offset();
        if distance == 0.0 {
            self.hide_tray_if_closed();
            return;
        }
        let reduce = objc2_ui_kit::UIAccessibilityIsReduceMotionEnabled();
        // UIKit's spring velocity is in distances per second; only toward
        // the target helps.
        let initial = (velocity / distance).clamp(0.0, 20.0);
        let (duration, damping, initial) = if reduce {
            (0.2, 1.0, 0.0)
        } else {
            (0.45, 0.86, initial)
        };
        let this = self.retain();
        let target = view.retain();
        let animations = RcBlock::new(move || this.move_to(&target, to));
        let this = self.retain();
        // A later move (the finger caught it) keeps the tray.
        let completion = RcBlock::new(move |_finished: Bool| this.hide_tray_if_closed());
        UIView::animateWithDuration_delay_usingSpringWithDamping_initialSpringVelocity_options_animations_completion(
            duration,
            0.0,
            damping,
            initial,
            UIViewAnimationOptions::AllowUserInteraction
                | UIViewAnimationOptions::BeginFromCurrentState,
            &animations,
            Some(&completion),
            view.mtm(),
        );
    }
}

impl Button {
    /// The symbol over the label, both centered in the content, which is
    /// as wide as the button's own width and as tall as the view. Laid out
    /// here rather than by autoresizing, which the content's first, empty
    /// frame would throw off.
    fn lay_out_content(&self, height: CGFloat) {
        let label = self.label.bounds().size;
        let icon = self
            .icon
            .as_ref()
            .map_or(CGSize::new(0.0, 0.0), |i| i.bounds().size);
        let gap = if self.icon.is_some() { ICON_GAP } else { 0.0 };
        let mut y = ((height - (icon.height + gap + label.height)) / 2.0).round();
        if let Some(view) = &self.icon {
            view.setFrame(CGRect::new(
                CGPoint::new(((self.width - icon.width) / 2.0).round(), y),
                icon,
            ));
            y += icon.height + gap;
        }
        self.label.setFrame(CGRect::new(
            CGPoint::new(((self.width - label.width) / 2.0).round(), y),
            label,
        ));
    }
}

/// Whether two buttons have the same fill: both destructive, both the
/// same tint, or both the edge's default.
fn same_fill(a: &SwipeButton, b: &SwipeButton) -> bool {
    match (a.destructive, b.destructive) {
        (true, true) => true,
        (false, false) => a.tint == b.tint,
        _ => false,
    }
}

/// Reports a pressed button's id as the root's `Menu` event, which runs
/// the action's handler in the core.
pub(crate) fn report(control: &EngineControl, id: u32) {
    if id == 0 {
        return;
    }
    control.dispatch_event(
        ROOT_ID,
        EventKind::Menu,
        EventPayload::Map(vec![("id", PayloadValue::Number(f64::from(id)))]),
    );
}

/// A button's color: the system's red when destructive, the app's tint,
/// else the system's blue at the leading edge and gray at the trailing
/// edge, as Mail's. System colors follow the appearance.
fn background(button: &SwipeButton, edge: SwipeEdge) -> Retained<UIColor> {
    if button.destructive {
        return UIColor::systemRedColor();
    }
    match (button.tint, edge) {
        (Some(color), _) => {
            let (r, g, b, a) = color.to_f64();
            UIColor::colorWithRed_green_blue_alpha(r, g, b, a)
        }
        (None, SwipeEdge::Leading) => UIColor::systemBlueColor(),
        (None, SwipeEdge::Trailing) => UIColor::systemGrayColor(),
    }
}

/// The SF Symbol `name` in white, as tall as a button's symbol: a white
/// view masked by the symbol's shape.
fn symbol_view(name: &str, mtm: MainThreadMarker) -> Option<Retained<UIView>> {
    let image = UIImage::systemImageNamed(&NSString::from_str(name))?;
    // `UIImageSymbolConfiguration` (iOS 13), by name: the crate's feature
    // for it is not enabled.
    let image = AnyClass::get(c"UIImageSymbolConfiguration")
        .and_then(|class| {
            // SAFETY: `+configurationWithPointSize:weight:` takes a CGFloat
            // and a UIImageSymbolWeight (NSInteger; 5 is medium).
            let configuration: Option<Retained<AnyObject>> =
                unsafe { msg_send![class, configurationWithPointSize: ICON_SIZE, weight: 5isize] };
            configuration
        })
        .and_then(|configuration| {
            // SAFETY: `-imageWithConfiguration:` of UIImage (iOS 13).
            let configured: Option<Retained<UIImage>> =
                unsafe { msg_send![&*image, imageWithConfiguration: &*configuration] };
            configured
        })
        .unwrap_or(image);
    // SAFETY: reading an image's size, scale and bitmap.
    let (size, scale, bitmap) = unsafe { (image.size(), image.scale(), image.CGImage()?) };
    let view = UIView::new(mtm);
    view.setTag(NO_NODE);
    view.setUserInteractionEnabled(false);
    view.setBackgroundColor(Some(&UIColor::whiteColor()));
    view.setFrame(CGRect::new(CGPoint::new(0.0, 0.0), size));
    let mask = CALayer::new();
    quiet(|| {
        mask.setFrame(CGRect::new(CGPoint::new(0.0, 0.0), size));
        mask.setContentsScale(scale);
        // SAFETY: a CGImage is a valid `contents` object.
        unsafe {
            mask.setContents(Some(
                &*(&*bitmap as *const objc2_core_graphics::CGImage as *const AnyObject),
            ))
        };
    });
    // SAFETY: a fresh mask layer owned by the view's layer.
    unsafe { view.layer().setMask(Some(&mask)) };
    Some(view)
}

fn remove_animations(view: &UIView) {
    view.layer().removeAllAnimations();
    for sub in view.subviews().iter() {
        remove_animations(&sub);
    }
}

/// A navigation controller's back gesture.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pop {
    /// From the screen's edge (`interactivePopGestureRecognizer`).
    Edge,
    /// From anywhere on the page (iOS 26's
    /// `interactiveContentPopGestureRecognizer`).
    Content,
}

/// Which back gesture `recognizer` is of a navigation controller showing
/// `view` (any up its responder chain, nested stacks included), if it is
/// one: found from the view, so it does not matter which of the
/// controller's views UIKit attached it to.
fn pop_of(recognizer: &UIGestureRecognizer, view: &UIView) -> Option<Pop> {
    let is = |r: Option<Retained<UIGestureRecognizer>>| {
        r.is_some_and(|r| std::ptr::eq::<UIGestureRecognizer>(&*r, recognizer))
    };
    let mut responder: Option<Retained<UIResponder>> = view.nextResponder();
    while let Some(r) = responder {
        if let Some(nav) = r
            .downcast_ref::<UIViewController>()
            .and_then(|c| c.navigationController())
        {
            if is(nav.interactivePopGestureRecognizer()) {
                return Some(Pop::Edge);
            }
            // Guarded by version: the property is new in iOS 26.
            if nav.respondsToSelector(sel!(interactiveContentPopGestureRecognizer))
                && is(nav.interactiveContentPopGestureRecognizer())
            {
                return Some(Pop::Content);
            }
        }
        responder = r.nextResponder();
    }
    None
}

/// Whether `recognizer` is one of a `UIContextMenuInteraction`'s on
/// `view`: on the view, with a context menu, and none of NSH's own (its
/// presses, its pans, this module's).
fn is_context_menu_recognizer(recognizer: &UIGestureRecognizer, view: &UIView) -> bool {
    let on_view = recognizer
        .view()
        .is_some_and(|v| std::ptr::eq::<UIView>(&*v, view));
    on_view
        && !recognizer.isKindOfClass(crate::press::PressRecognizer::class())
        && !recognizer.delegate().is_some_and(|d| {
            let d: &AnyObject = d.as_ref();
            d.downcast_ref::<SwipeRow>().is_some() || d.downcast_ref::<EventTarget>().is_some()
        })
        && view.interactions().iter().any(|i| {
            AsRef::<AnyObject>::as_ref(&*i)
                .downcast_ref::<objc2_ui_kit::UIContextMenuInteraction>()
                .is_some()
        })
}

/// Cancels the context menu the long press on `view` was going to show: a
/// swipe started first. Turning the menu's recognizers off and on cancels
/// them, and UIKit undoes its press highlight as for any cancelled touch
/// (taking the interaction off mid-press left the row highlighted).
fn cancel_context_menu(view: &UIView) {
    let Some(recognizers) = view.gestureRecognizers() else {
        return;
    };
    for r in recognizers.iter() {
        if is_context_menu_recognizer(&r, view) && r.isEnabled() {
            r.setEnabled(false);
            r.setEnabled(true);
        }
    }
}

/// A context menu showed for the touch on `view`: the views with swipe
/// actions under it (`view` and those around it) stop following it, so a
/// sideways drag on the menu's preview does not swipe them. Turning a pan
/// off and on cancels it for the rest of the touch.
pub(crate) fn context_menu_shown(view: &UIView) {
    let mut current = Some(view.retain());
    while let Some(v) = current {
        if let Some(row) = v.downcast_ref::<NshView>().and_then(NshView::swipe)
            && let Some(pan) = row.ivars().pan.borrow().as_ref()
            && pan.isEnabled()
        {
            pan.setEnabled(false);
            pan.setEnabled(true);
        }
        current = v.superview();
    }
}

/// Whether `recognizer` is the pan of a scroll view around `view`.
fn is_scroll_pan_around(recognizer: &UIGestureRecognizer, view: &UIView) -> bool {
    let Some(owner) = recognizer.view() else {
        return false;
    };
    let Some(scroll) = owner.downcast_ref::<UIScrollView>() else {
        return false;
    };
    view.isDescendantOfView(scroll)
        && std::ptr::eq::<UIGestureRecognizer>(&**scroll.panGestureRecognizer(), recognizer)
}

/// Whether a move toward going back (`moving_right`, mirrored right to
/// left) is the navigation's: it started within [`BACK_EDGE`] of the
/// window's leading edge, and the stack around `view` can go back.
fn yields_to_back(
    pan: &UIPanGestureRecognizer,
    view: &UIView,
    moving_right: bool,
    rtl: bool,
) -> bool {
    if moving_right == rtl {
        return false;
    }
    let Some(window) = view.window() else {
        return false;
    };
    let start = pan.locationInView(None).x - pan.translationInView(None).x;
    let from_edge = if rtl {
        window.bounds().size.width - start
    } else {
        start
    };
    from_edge < BACK_EDGE && can_go_back(view)
}

/// Whether the navigation controller showing `view` has a page to go back
/// to.
fn can_go_back(view: &UIView) -> bool {
    let mut responder: Option<Retained<UIResponder>> = view.nextResponder();
    while let Some(r) = responder {
        if let Some(controller) = r.downcast_ref::<UIViewController>()
            && let Some(nav) = controller.navigationController()
        {
            return nav.viewControllers().count() > 1;
        }
        responder = r.nextResponder();
    }
    false
}

/// Whether `a` and `b` are in the same scroll view (the nearest around
/// each): a press beside an open view in its list only closes it.
fn same_scroll_view(a: &UIView, b: &UIView) -> bool {
    let scroll_of = |v: &UIView| {
        let mut current = v.superview();
        while let Some(s) = current {
            if s.isKindOfClass(UIScrollView::class()) {
                return Some(s);
            }
            current = s.superview();
        }
        None
    };
    match (scroll_of(a), scroll_of(b)) {
        (Some(x), Some(y)) => std::ptr::eq::<UIView>(&*x, &*y),
        _ => false,
    }
}
