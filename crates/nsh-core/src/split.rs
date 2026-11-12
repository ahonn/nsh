//! Split views: two or three columns side by side, each normally a
//! navigation stack of its own, as Mail, Notes and Files show them on an
//! iPad (Rust apps: `nsh_base::split_view`, the core's
//! [`crate::element::SplitView`]). This module holds what the platforms
//! see ([`SplitConfig`] in [`crate::ViewProps::split`]) and the layout of
//! the platforms that have no split view of their own.
//!
//! iOS (Mac Catalyst too) shows a split view as a column-style
//! `UISplitViewController`: double column with a primary and a secondary
//! column, triple column with a supplementary column between them. The
//! nav stack of each column is the column's view controller, so every
//! column has its own system navigation bar. UIKit decides which columns
//! show and how wide they are from the preferences here and the room it
//! has (rotation, Split View, Slide Over), reports the user's changes as
//! [`crate::EventKind::DisplayModeChange`] and lays each column's pages
//! out at the column's real width (`Core::set_stack_size`).
//!
//! The other platforms lay the split view out as a row: the primary and
//! supplementary columns as wide as their preferred widths, the secondary
//! column taking the rest, and the columns the preferred display mode
//! leaves out hidden ([`SplitDisplayMode::shows`]).

use crate::{
    NodeId,
    geometry::{Points, px},
};

/// One column of a split view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SplitColumn {
    /// The leading column: a sidebar.
    #[default]
    Primary,
    /// The middle column of a triple-column split view: a list.
    Supplementary,
    /// The trailing column, the widest: the detail.
    Secondary,
}

/// Which columns a split view shows and how (UIKit's
/// `UISplitViewController.DisplayMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SplitDisplayMode {
    /// The platform decides from the room it has (iPad landscape: every
    /// column side by side; portrait: the secondary column with the
    /// supplementary one beside it, the primary column slid over on
    /// request).
    #[default]
    Automatic,
    /// The secondary column alone.
    SecondaryOnly,
    /// One column beside the secondary one: the supplementary column in a
    /// triple-column split view, else the primary one.
    OneBesideSecondary,
    /// One column over the secondary one, which keeps its width.
    OneOverSecondary,
    /// Both other columns beside the secondary one (triple column).
    TwoBesideSecondary,
    /// Both other columns over the secondary one (triple column).
    TwoOverSecondary,
    /// Both other columns pushing the secondary one aside, partly off
    /// screen (triple column).
    TwoDisplaceSecondary,
}

impl SplitDisplayMode {
    /// Every mode, in UIKit's order.
    pub const ALL: [SplitDisplayMode; 7] = [
        SplitDisplayMode::Automatic,
        SplitDisplayMode::SecondaryOnly,
        SplitDisplayMode::OneBesideSecondary,
        SplitDisplayMode::OneOverSecondary,
        SplitDisplayMode::TwoBesideSecondary,
        SplitDisplayMode::TwoOverSecondary,
        SplitDisplayMode::TwoDisplaceSecondary,
    ];

    /// Its name in events (`"automatic"`, `"secondaryOnly"`,
    /// `"oneBesideSecondary"`, ...).
    pub fn name(self) -> &'static str {
        match self {
            SplitDisplayMode::Automatic => "automatic",
            SplitDisplayMode::SecondaryOnly => "secondaryOnly",
            SplitDisplayMode::OneBesideSecondary => "oneBesideSecondary",
            SplitDisplayMode::OneOverSecondary => "oneOverSecondary",
            SplitDisplayMode::TwoBesideSecondary => "twoBesideSecondary",
            SplitDisplayMode::TwoOverSecondary => "twoOverSecondary",
            SplitDisplayMode::TwoDisplaceSecondary => "twoDisplaceSecondary",
        }
    }

    /// The mode named `name` ([`Self::name`]).
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.name() == name)
    }

    /// Whether `column` shows in this mode, where the platform lays the
    /// split view out as a row (every column with
    /// [`SplitDisplayMode::Automatic`]); `triple`: the split view has a
    /// supplementary column. A column over or pushing the secondary one
    /// shows beside it there.
    pub fn shows(self, column: SplitColumn, triple: bool) -> bool {
        use SplitDisplayMode as M;
        match (self, column) {
            (_, SplitColumn::Secondary) | (M::Automatic, _) => true,
            (M::SecondaryOnly, _) => false,
            (_, SplitColumn::Supplementary) => triple,
            (M::OneBesideSecondary | M::OneOverSecondary, SplitColumn::Primary) => !triple,
            (_, SplitColumn::Primary) => true,
        }
    }
}

/// How the secondary column makes room for the others (UIKit's
/// `UISplitViewController.SplitBehavior`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SplitBehavior {
    /// The platform decides from the room it has.
    #[default]
    Automatic,
    /// The columns sit side by side.
    Tile,
    /// The other columns cover the secondary one.
    Overlay,
    /// The other columns push the secondary one aside.
    Displace,
}

/// Whether the system's button that shows and hides the sidebar (UIKit's
/// `displayModeButtonItem`, in the header of the column next to the
/// hidden one) shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SplitButtonVisibility {
    /// Where the platform shows it (UIKit: whenever a column can be shown
    /// or hidden).
    #[default]
    Automatic,
    /// Always.
    Always,
    /// Never: the app shows and hides the columns itself.
    Never,
}

/// The width of a column the app prefers, and the range it may take.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub struct ColumnWidth {
    /// The width it prefers; `None`: the platform's.
    pub preferred: Option<Points>,
    /// The narrowest it may be; `None`: the platform's.
    pub min: Option<Points>,
    /// The widest it may be; `None`: the platform's.
    pub max: Option<Points>,
}

impl ColumnWidth {
    /// The width it takes where the platform lays the split view out as a
    /// row: its preferred width (else `default`) within its range.
    pub fn resolved(self, default: Points) -> Points {
        let mut width = self.preferred.unwrap_or(default);
        if let Some(max) = self.max {
            width = width.min(max);
        }
        if let Some(min) = self.min {
            width = width.max(min);
        }
        width
    }
}

/// What the platforms take of a split view (a view's
/// [`crate::ViewProps::split`]).
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct SplitConfig {
    /// The columns' display mode the app prefers; the platform may show
    /// another (and reports it).
    pub display_mode: SplitDisplayMode,
    /// How the secondary column makes room for the others.
    pub behavior: SplitBehavior,
    /// The primary column's width.
    pub primary_width: ColumnWidth,
    /// The supplementary column's width.
    pub supplementary_width: ColumnWidth,
    /// Whether the system's sidebar button shows.
    pub button: SplitButtonVisibility,
    /// The nav stack of each column, in order (primary, supplementary in a
    /// triple-column split view, secondary): the first stack inside the
    /// split view's child for the column (`None`: the child holds none,
    /// and the column shows nothing on iOS). Set by the core, from the
    /// split view's children.
    pub columns: Vec<Option<NodeId>>,
}

impl SplitConfig {
    /// The primary column's width where the platform lays the split view
    /// out as a row and the app prefers none: a sidebar's.
    pub const PRIMARY_WIDTH: Points = px(280.0);
    /// The supplementary column's width where the platform lays the split
    /// view out as a row and the app prefers none: a list's.
    pub const SUPPLEMENTARY_WIDTH: Points = px(320.0);

    /// The split view has a supplementary column (three columns).
    pub fn triple(&self) -> bool {
        self.columns.len() >= 3
    }

    /// The nav stack of `column`, if the split view has that column and it
    /// holds one.
    pub fn column(&self, column: SplitColumn) -> Option<NodeId> {
        let index = match (column, self.triple()) {
            (SplitColumn::Primary, _) => 0,
            (SplitColumn::Supplementary, true) => 1,
            (SplitColumn::Supplementary, false) => return None,
            (SplitColumn::Secondary, true) => 2,
            (SplitColumn::Secondary, false) => 1,
        };
        self.columns.get(index).copied().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_display_mode_shows_its_columns_in_a_row() {
        use SplitColumn::*;
        use SplitDisplayMode as M;
        let shown = |mode: M, triple: bool| {
            let columns: &[SplitColumn] = if triple {
                &[Primary, Supplementary, Secondary]
            } else {
                &[Primary, Secondary]
            };
            columns
                .iter()
                .filter(|&&c| mode.shows(c, triple))
                .copied()
                .collect::<Vec<_>>()
        };
        assert_eq!(
            shown(M::Automatic, true),
            [Primary, Supplementary, Secondary]
        );
        assert_eq!(shown(M::SecondaryOnly, true), [Secondary]);
        assert_eq!(
            shown(M::OneBesideSecondary, true),
            [Supplementary, Secondary]
        );
        assert_eq!(shown(M::OneOverSecondary, false), [Primary, Secondary]);
        assert_eq!(
            shown(M::TwoDisplaceSecondary, true),
            [Primary, Supplementary, Secondary]
        );
        assert_eq!(shown(M::TwoBesideSecondary, false), [Primary, Secondary]);
    }

    #[test]
    fn modes_are_named_as_events_report_them() {
        for mode in SplitDisplayMode::ALL {
            assert_eq!(SplitDisplayMode::from_name(mode.name()), Some(mode));
        }
        assert_eq!(SplitDisplayMode::from_name("allVisible"), None);
    }

    #[test]
    fn a_column_width_keeps_to_its_range() {
        let width = ColumnWidth {
            preferred: Some(px(500.0)),
            max: Some(px(400.0)),
            ..ColumnWidth::default()
        };
        assert_eq!(width.resolved(px(280.0)), px(400.0));
        let width = ColumnWidth {
            min: Some(px(300.0)),
            ..ColumnWidth::default()
        };
        assert_eq!(width.resolved(px(280.0)), px(300.0));
    }

    #[test]
    fn columns_are_found_by_role() {
        let config = SplitConfig {
            columns: vec![Some(2), None, Some(6)],
            ..SplitConfig::default()
        };
        assert_eq!(config.column(SplitColumn::Primary), Some(2));
        assert_eq!(config.column(SplitColumn::Supplementary), None);
        assert_eq!(config.column(SplitColumn::Secondary), Some(6));
        let config = SplitConfig {
            columns: vec![Some(2), Some(4)],
            ..SplitConfig::default()
        };
        assert_eq!(config.column(SplitColumn::Supplementary), None);
        assert_eq!(config.column(SplitColumn::Secondary), Some(4));
    }
}
