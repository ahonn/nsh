//! Split views ([`nsh_core::split`]): a column-style `UISplitViewController`
//! whose columns are the navigation controllers of the split view's nav
//! stacks, so each column keeps its own pages, swipe back and system
//! navigation bar.
//!
//! The core makes a split view a plain view and sends its options in the
//! view's props ahead of its columns ([`nsh_core::ViewProps::split`]); the
//! mounter then keeps the view's children out of its subviews (they are
//! columns) and gives each column's stack to the split view controller at
//! the end of the batch. The controller's view fills the core's view, and
//! the controller is a child of the controller showing the surface. UIKit
//! decides which columns show and how wide they are; each column's stack
//! reports its size, at which the core lays its pages out
//! (`NavController::report_column_size`), and the user's changes of display
//! mode reach the app as [`EventKind::DisplayModeChange`].
//!
//! The sidebar (the primary column) is UIKit's to draw: its pages' views
//! are clear so the system's sidebar material shows (iPadOS 26's floating
//! glass sidebar).
//!
//! In a compact width the controller collapses as UIKit does by default,
//! its columns' pages onto the primary column's stack. Changes the app
//! makes to the columns' stacks meanwhile wait until it expands again
//! (`Mounter::finish_navigation`); apps show another layout there instead
//! (`horizontal_size_class()`).

use nsh_core::{
    EventKind, EventPayload, NodeId, PayloadValue,
    split::{SplitBehavior, SplitButtonVisibility, SplitColumn, SplitConfig, SplitDisplayMode},
};
use nsh_engine::EngineControl;
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, rc::Retained,
    runtime::ProtocolObject,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSObject, NSObjectProtocol};
use objc2_ui_kit::{
    UISplitViewController, UISplitViewControllerAutomaticDimension, UISplitViewControllerColumn,
    UISplitViewControllerDelegate, UISplitViewControllerDisplayMode,
    UISplitViewControllerDisplayModeButtonVisibility, UISplitViewControllerSplitBehavior,
    UISplitViewControllerStyle, UIView, UIViewAutoresizing, UIViewController,
};

use crate::{mount::SharedMounter, navigation::controller_of};

pub(crate) struct DelegateIvars {
    split: NodeId,
    control: EngineControl,
    mounter: SharedMounter,
}

define_class!(
    /// Reports the display modes UIKit moves the split view to, and catches
    /// up on the columns' changes once it expands.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = DelegateIvars]
    pub(crate) struct SplitDelegate;

    unsafe impl NSObjectProtocol for SplitDelegate {}

    unsafe impl UISplitViewControllerDelegate for SplitDelegate {
        #[unsafe(method(splitViewController:willChangeToDisplayMode:))]
        fn will_change_to_display_mode(
            &self,
            _split: &UISplitViewController,
            mode: UISplitViewControllerDisplayMode,
        ) {
            let ivars = self.ivars();
            ivars.control.dispatch_event(
                ivars.split,
                EventKind::DisplayModeChange,
                EventPayload::Map(vec![(
                    "displayMode",
                    PayloadValue::Str(display_mode_of(mode).name().to_owned()),
                )]),
            );
        }

        #[unsafe(method(splitViewControllerDidExpand:))]
        fn did_expand(&self, _split: &UISplitViewController) {
            crate::mount::Mounter::split_expanded(&self.ivars().mounter, self.ivars().split);
        }
    }
);

impl SplitDelegate {
    fn new(
        mtm: MainThreadMarker,
        split: NodeId,
        control: EngineControl,
        mounter: SharedMounter,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DelegateIvars {
            split,
            control,
            mounter,
        });
        // SAFETY: NSObject's designated initializer.
        unsafe { msg_send![super(this), init] }
    }
}

/// A split view's controller and the columns it was given.
pub(crate) struct SplitHost {
    pub(crate) controller: Retained<UISplitViewController>,
    _delegate: Retained<SplitDelegate>,
    /// Made with a supplementary column (UIKit fixes the style at creation).
    pub(crate) triple: bool,
    /// The options last applied (the columns among them).
    pub(crate) config: SplitConfig,
    /// The stack each column was given: primary, supplementary, secondary.
    pub(crate) assigned: [Option<NodeId>; 3],
    /// What a column without a stack shows: nothing.
    empty: [Option<Retained<UIViewController>>; 3],
    /// The controller is a child of the controller showing the surface.
    contained: bool,
}

/// The index of `column` in [`SplitHost::assigned`].
pub(crate) fn index(column: SplitColumn) -> usize {
    match column {
        SplitColumn::Primary => 0,
        SplitColumn::Supplementary => 1,
        SplitColumn::Secondary => 2,
    }
}

fn uikit_column(column: SplitColumn) -> UISplitViewControllerColumn {
    match column {
        SplitColumn::Primary => UISplitViewControllerColumn::Primary,
        SplitColumn::Supplementary => UISplitViewControllerColumn::Supplementary,
        SplitColumn::Secondary => UISplitViewControllerColumn::Secondary,
    }
}

fn uikit_display_mode(mode: SplitDisplayMode) -> UISplitViewControllerDisplayMode {
    match mode {
        SplitDisplayMode::Automatic => UISplitViewControllerDisplayMode::Automatic,
        SplitDisplayMode::SecondaryOnly => UISplitViewControllerDisplayMode::SecondaryOnly,
        SplitDisplayMode::OneBesideSecondary => {
            UISplitViewControllerDisplayMode::OneBesideSecondary
        }
        SplitDisplayMode::OneOverSecondary => UISplitViewControllerDisplayMode::OneOverSecondary,
        SplitDisplayMode::TwoBesideSecondary => {
            UISplitViewControllerDisplayMode::TwoBesideSecondary
        }
        SplitDisplayMode::TwoOverSecondary => UISplitViewControllerDisplayMode::TwoOverSecondary,
        SplitDisplayMode::TwoDisplaceSecondary => {
            UISplitViewControllerDisplayMode::TwoDisplaceSecondary
        }
    }
}

fn display_mode_of(mode: UISplitViewControllerDisplayMode) -> SplitDisplayMode {
    SplitDisplayMode::ALL
        .into_iter()
        .find(|&m| uikit_display_mode(m) == mode)
        .unwrap_or_default()
}

/// A width the app set, else UIKit's own.
fn dimension(width: Option<nsh_core::Points>) -> CGFloat {
    // SAFETY: a UIKit constant.
    width.map_or(unsafe { UISplitViewControllerAutomaticDimension }, |w| {
        CGFloat::from(w.0)
    })
}

impl SplitHost {
    /// A split view controller filling `container`, with three columns
    /// when `triple`.
    pub(crate) fn new(
        mtm: MainThreadMarker,
        id: NodeId,
        container: &UIView,
        triple: bool,
        control: EngineControl,
        mounter: SharedMounter,
    ) -> Self {
        let style = if triple {
            UISplitViewControllerStyle::TripleColumn
        } else {
            UISplitViewControllerStyle::DoubleColumn
        };
        let controller =
            UISplitViewController::initWithStyle(UISplitViewController::alloc(mtm), style);
        let delegate = SplitDelegate::new(mtm, id, control, mounter);
        controller.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        if let Some(view) = controller.view() {
            view.setAutoresizingMask(
                UIViewAutoresizing::FlexibleWidth | UIViewAutoresizing::FlexibleHeight,
            );
            view.setFrame(container.bounds());
            container.addSubview(&view);
        }
        Self {
            controller,
            _delegate: delegate,
            triple,
            config: SplitConfig::default(),
            assigned: [None; 3],
            empty: [None, None, None],
            contained: false,
        }
    }

    /// Applies the options of `config` (its columns are given at the end
    /// of the batch, `Mounter::finish_splits`). A display mode that
    /// changes once the split view shows moves to its columns animated.
    pub(crate) fn configure(&mut self, config: SplitConfig) {
        let c = &self.controller;
        let old = &self.config;
        if config.behavior != old.behavior || !self.contained {
            c.setPreferredSplitBehavior(match config.behavior {
                SplitBehavior::Automatic => UISplitViewControllerSplitBehavior::Automatic,
                SplitBehavior::Tile => UISplitViewControllerSplitBehavior::Tile,
                SplitBehavior::Overlay => UISplitViewControllerSplitBehavior::Overlay,
                SplitBehavior::Displace => UISplitViewControllerSplitBehavior::Displace,
            });
        }
        c.setDisplayModeButtonVisibility(match config.button {
            SplitButtonVisibility::Automatic => {
                UISplitViewControllerDisplayModeButtonVisibility::Automatic
            }
            SplitButtonVisibility::Always => {
                UISplitViewControllerDisplayModeButtonVisibility::Always
            }
            SplitButtonVisibility::Never => UISplitViewControllerDisplayModeButtonVisibility::Never,
        });
        let primary = config.primary_width;
        c.setPreferredPrimaryColumnWidth(dimension(primary.preferred));
        c.setMinimumPrimaryColumnWidth(dimension(primary.min));
        c.setMaximumPrimaryColumnWidth(dimension(primary.max));
        if self.triple {
            let supplementary = config.supplementary_width;
            c.setPreferredSupplementaryColumnWidth(dimension(supplementary.preferred));
            c.setMinimumSupplementaryColumnWidth(dimension(supplementary.min));
            c.setMaximumSupplementaryColumnWidth(dimension(supplementary.max));
        }
        let mode = uikit_display_mode(config.display_mode);
        if !self.contained || c.preferredDisplayMode() == mode {
            c.setPreferredDisplayMode(mode);
        } else {
            let controller = c.clone();
            let block = block2::RcBlock::new(move || controller.setPreferredDisplayMode(mode));
            UIView::animateWithDuration_animations(0.3, &block, c.mtm());
        }
        self.config = config;
    }

    /// Gives `column` the navigation controller `nav` (`None`: an empty
    /// column).
    pub(crate) fn set_column(&mut self, column: SplitColumn, nav: Option<&UIViewController>) {
        let mtm = self.controller.mtm();
        let controller = match nav {
            Some(nav) => nav.retain(),
            None => self.empty[index(column)]
                .get_or_insert_with(|| UIViewController::new(mtm))
                .clone(),
        };
        self.controller
            .setViewController_forColumn(Some(&controller), uikit_column(column));
    }

    /// Makes the controller a child of the controller showing `container`,
    /// once it is in a window (appearance callbacks, safe areas,
    /// rotation, size classes).
    pub(crate) fn contain(&mut self, container: &UIView) {
        if self.contained || container.window().is_none() {
            return;
        }
        let Some(parent) = controller_of(container) else {
            return;
        };
        parent.addChildViewController(&self.controller);
        self.controller.didMoveToParentViewController(Some(&parent));
        self.contained = true;
    }

    /// Shows or hides `column` (UIKit's `showColumn:` / `hideColumn:`).
    pub(crate) fn show_column(&self, column: SplitColumn, shown: bool) {
        if column == SplitColumn::Supplementary && !self.triple {
            return;
        }
        if shown {
            self.controller.showColumn(uikit_column(column));
        } else {
            self.controller.hideColumn(uikit_column(column));
        }
    }

    /// Takes the controller out of the surface: the split view goes.
    pub(crate) fn remove(&mut self) {
        if self.contained {
            self.controller.willMoveToParentViewController(None);
        }
        if let Some(view) = self.controller.viewIfLoaded() {
            view.removeFromSuperview();
        }
        if self.contained {
            self.controller.removeFromParentViewController();
            self.contained = false;
        }
    }
}
