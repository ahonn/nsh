//! The system's share sheet ([`crate::element::share`]): the app hands
//! the user text, links and files, and the user picks where they go (a
//! message, a mail, AirDrop, Save to Files, another app's extension).
//!
//! The flow, every part of it in Rust: the call checks the items
//! ([`check`]); the core keeps the request and sends
//! [`Command::Share`](crate::Command::Share) on the root, with the view
//! to anchor a popover to; the platform shows its sheet (iOS and Mac
//! Catalyst: `UIActivityViewController` over the top presented controller)
//! and reports how it closed as the root's
//! [`EventKind::Share`](crate::EventKind::Share) (`id`, `completed`,
//! `activity`), once. A platform without a share sheet
//! ([`Environment::share`](crate::platform::Environment::share) unset) gets
//! no request: the core answers "not completed" at once.

use std::path::{Path, PathBuf};

/// Something to share ([`crate::element::share`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ShareItem {
    /// Plain text: a message, a mail's body, a note.
    Text(String),
    /// A link: an absolute URL, not `file:` (a local file is a
    /// [`ShareItem::File`]). Shared as a URL, so targets can show its
    /// preview.
    Url(String),
    /// A local file: an absolute path to an existing regular file, shared
    /// as the file itself under its name (a mail's attachment, AirDrop,
    /// Save to Files). Write it before sharing (a cache or temporary
    /// directory) and keep it until the share's answer comes.
    File(PathBuf),
}

impl ShareItem {
    /// Plain text ([`ShareItem::Text`]).
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    /// A link ([`ShareItem::Url`]).
    pub fn url(url: impl Into<String>) -> Self {
        Self::Url(url.into())
    }

    /// A local file ([`ShareItem::File`]).
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self::File(path.into())
    }
}

/// How a share sheet closed ([`crate::element::share`]).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct ShareResult {
    /// The user shared: the activity they chose ran to its end. `false`
    /// when they closed the sheet or cancelled the activity, and where the
    /// platform has no share sheet
    /// ([`crate::element::can_share`]).
    pub completed: bool,
    /// The activity the user chose, as the platform names it (iOS's
    /// `UIActivityType`: `com.apple.UIKit.activity.Mail`, an app
    /// extension's bundle identifier); `None` when they chose none.
    pub activity: Option<String>,
}

/// Checks `items` before they go to the platform: at least one, links
/// absolute URLs other than `file:`, files absolute paths to existing
/// regular files. The error says what is wrong.
pub(crate) fn check(items: &[ShareItem]) -> Result<(), String> {
    if items.is_empty() {
        return Err("share: nothing to share".into());
    }
    for item in items {
        match item {
            ShareItem::Text(_) => {}
            ShareItem::Url(url) => check_url(url)?,
            ShareItem::File(path) => check_file(path)?,
        }
    }
    Ok(())
}

fn check_url(url: &str) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|e| format!("share: `{url}` is no URL: {e}"))?;
    if parsed.scheme() == "file" {
        return Err(format!(
            "share: `{url}` is a local file: share it with `ShareItem::file`"
        ));
    }
    Ok(())
}

fn check_file(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err(format!(
            "share: `{}` is not an absolute path",
            path.display()
        ));
    }
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => Ok(()),
        Ok(_) => Err(format!("share: `{}` is not a file", path.display())),
        Err(e) => Err(format!("share: `{}`: {e}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_are_checked_before_they_reach_the_platform() {
        assert!(check(&[]).is_err());
        assert!(check(&[ShareItem::text("")]).is_ok());
        assert!(check(&[ShareItem::url("https://example.com/a?b=c")]).is_ok());
        assert!(check(&[ShareItem::url("mailto:someone@example.com")]).is_ok());
        assert!(check(&[ShareItem::url("example.com")]).is_err());
        assert!(check(&[ShareItem::url("file:///etc/hosts")]).is_err());
        assert!(check(&[ShareItem::file("relative.csv")]).is_err());

        let dir = std::env::temp_dir();
        assert!(check(&[ShareItem::file(&dir)]).is_err(), "a directory");
        let missing = dir.join("nsh-share-check-missing.csv");
        assert!(check(&[ShareItem::file(&missing)]).is_err());
        let file = dir.join(format!("nsh-share-check-{}.csv", std::process::id()));
        std::fs::write(&file, "a,b\n").unwrap();
        let checked = check(&[ShareItem::text("export"), ShareItem::file(&file)]);
        let _ = std::fs::remove_file(&file);
        assert_eq!(checked, Ok(()));
    }
}
