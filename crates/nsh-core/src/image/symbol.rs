//! System symbols as image sources: `symbol:<name>` names one of Apple's
//! SF Symbols (`symbol:checkmark.circle.fill`), with an optional weight
//! (`symbol:star?weight=600`). An image view shows it as any image, so its
//! `tintColor`, `objectFit`, `objectPosition`, filters and accessibility
//! apply; a symbol is contained in its box unless `objectFit` says
//! otherwise (an icon shows whole).
//!
//! The symbol is drawn by the platform, never loaded: no read, download,
//! disk cache or access check is involved (`symbol:` URIs are not URLs, so
//! [`crate::access::Access::allows_media`] lets them through). iOS and
//! macOS draw it at the size the view shows it, in black with its alpha
//! (the tint shows through that alpha, as for any template image); the
//! other platforms have no SF Symbols and report the load as failed.

use std::fmt;

use crate::element::FontWeight;

/// An SF Symbol to show as an image ([`crate::element::image`]):
/// its name and weight. It converts into an
/// [`ImageSrc`](crate::element::ImageSrc), and parses from and writes the
/// `symbol:` URI that carries it to the platforms.
///
/// ```
/// use nsh_core::{element::image, image::Symbol, prelude::*};
///
/// let check = image(Symbol::new("checkmark.circle.fill"))
///     .size(px(20.))
///     .tint_color(rgb(0x34c759));
/// let star = image(Symbol::new("star").weight(FontWeight::SEMIBOLD));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub struct Symbol {
    /// The symbol's name, as the SF Symbols app shows it
    /// (`checkmark.circle.fill`).
    pub name: String,
    /// The weight of its strokes, as a font's: SF Symbols come in nine,
    /// from [`FontWeight::THIN`] (100, ultralight) to [`FontWeight::BLACK`]
    /// (900); other values take the nearest. `None`: regular.
    pub weight: Option<FontWeight>,
}

impl Symbol {
    /// The URI scheme of symbol sources.
    pub const SCHEME: &'static str = "symbol:";

    /// The symbol `name`, at the regular weight.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            weight: None,
        }
    }

    /// Draws its strokes at `weight`.
    #[must_use]
    pub fn weight(mut self, weight: FontWeight) -> Self {
        self.weight = Some(weight);
        self
    }

    /// Whether `uri` names a symbol (cheap: its scheme only).
    pub fn is_symbol(uri: &str) -> bool {
        uri.starts_with(Self::SCHEME)
    }

    /// The symbol a `symbol:` URI names; `None` for other URIs and for one
    /// without a name. Parameters after `?` are `&`-separated `key=value`
    /// pairs: `weight` (a font weight, 1 to 1000); unknown keys and values
    /// that do not parse are ignored, so a URI written for a later version
    /// still shows its symbol.
    pub fn parse(uri: &str) -> Option<Self> {
        let rest = uri.strip_prefix(Self::SCHEME)?;
        let (name, params) = rest.split_once('?').unwrap_or((rest, ""));
        if name.is_empty() || name.contains(char::is_whitespace) {
            return None;
        }
        let mut symbol = Self::new(name);
        for (key, value) in params.split('&').filter_map(|p| p.split_once('=')) {
            if key == "weight" {
                symbol.weight = value
                    .parse::<u16>()
                    .ok()
                    .filter(|w| (1..=1000).contains(w))
                    .map(FontWeight);
            }
        }
        Some(symbol)
    }

    /// Its weight as one of SF Symbols' nine, 1 (ultralight) to 9 (black),
    /// the nearest to [`Self::weight`]; `None` when it has none (regular).
    /// UIKit's `UIImageSymbolWeight` numbers them so.
    pub fn weight_step(&self) -> Option<u8> {
        self.weight
            .map(|w| ((u32::from(w.0) + 50) / 100).clamp(1, 9) as u8)
    }
}

impl fmt::Display for Symbol {
    /// Its `symbol:` URI.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", Self::SCHEME, self.name)?;
        if let Some(weight) = self.weight {
            write!(f, "?weight={}", weight.0)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_names_and_weights() {
        assert_eq!(
            Symbol::parse("symbol:checkmark.circle.fill"),
            Some(Symbol::new("checkmark.circle.fill"))
        );
        assert_eq!(
            Symbol::parse("symbol:star?weight=600"),
            Some(Symbol::new("star").weight(FontWeight::SEMIBOLD))
        );
        // Unknown keys and unreadable values are ignored.
        assert_eq!(
            Symbol::parse("symbol:star?scale=large&weight=bold"),
            Some(Symbol::new("star"))
        );
        assert_eq!(
            Symbol::parse("symbol:star?weight=0"),
            Some(Symbol::new("star"))
        );
        assert_eq!(Symbol::parse("symbol:"), None);
        assert_eq!(Symbol::parse("symbol:?weight=400"), None);
        assert_eq!(Symbol::parse("symbol:a b"), None);
        assert_eq!(Symbol::parse("asset:/star.png"), None);
        assert!(Symbol::is_symbol("symbol:star"));
        assert!(!Symbol::is_symbol("https://example.com/symbol:star"));
    }

    #[test]
    fn writes_the_uri_it_parses() {
        for symbol in [
            Symbol::new("trash"),
            Symbol::new("person.crop.circle").weight(FontWeight::BOLD),
        ] {
            let uri = symbol.to_string();
            assert_eq!(Symbol::parse(&uri), Some(symbol));
        }
        assert_eq!(
            Symbol::new("star").weight(FontWeight(650)).to_string(),
            "symbol:star?weight=650"
        );
    }

    #[test]
    fn weights_round_to_the_nearest_of_nine() {
        let step = |w: u16| Symbol::new("a").weight(FontWeight(w)).weight_step();
        assert_eq!(Symbol::new("a").weight_step(), None);
        assert_eq!(step(1), Some(1));
        assert_eq!(step(100), Some(1));
        assert_eq!(step(400), Some(4));
        assert_eq!(step(549), Some(5));
        assert_eq!(step(550), Some(6));
        assert_eq!(step(900), Some(9));
        assert_eq!(step(1000), Some(9));
    }
}
