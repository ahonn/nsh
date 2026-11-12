//! The Rust gallery (`examples/gallery-rs`): an app written in Rust. Its
//! runners (`nsh.json` `"root"`) and the native executable mount
//! [`Gallery`], and everything it shows is written in Rust: the same demos
//! as the TSX gallery (`examples/gallery`), on nsh's Rust component API.

mod assets;
mod demos;
mod nav;
mod shell;
mod ui;

use nsh_core::prelude::*;

use crate::ui::{colors, desktop};

/// The UI set of the gallery (`nsh.json` `"rust"`): the files it ships.
#[native_module("gallery/native")]
fn native(registry: &mut Registry) {
    registry.assets(assets!());
}

/// The gallery, in the gallery's design for the system's appearance: on
/// phones a native stack of its index and the screens pushed on it; in wide
/// windows a sidebar of the demos and the selected one beside it; the demo
/// open when it last ran opens again.
#[component]
pub fn Gallery() -> impl IntoElement {
    shell::restore();
    let split = global::<demos::split::SplitOpen>();
    view().flex_1().bg(colors::BACKGROUND).child(
        // The split view demo takes the window, as an app's root layout.
        when(
            move || split.get().0,
            || demos::split::mail_split().into_element(),
        )
        .otherwise(|| {
            // Made again when the window crosses the threshold, as TS
            // remounts its shell: what is open carries over.
            when(desktop, shell::desktop_layout).otherwise(shell::phone)
        }),
    )
}
