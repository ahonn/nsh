//! SF Symbols as images ([`nsh_core::image::Symbol`], `symbol:<name>`):
//! drawn by AppKit (macOS 11 and later) on the main thread when a view
//! asks, never loaded. The symbol is drawn in black with its own alpha, so
//! the view's `tintColor` (a mask of the image, [`crate::decor`]) colors it
//! as any template image.

use nsh_core::{
    geometry::{Points, Size, size},
    image::Symbol,
};
use objc2::{ClassType, sel};
use objc2_app_kit::{
    NSFontWeight, NSFontWeightBlack, NSFontWeightBold, NSFontWeightHeavy, NSFontWeightLight,
    NSFontWeightMedium, NSFontWeightRegular, NSFontWeightSemibold, NSFontWeightThin,
    NSFontWeightUltraLight, NSGraphicsContext, NSImage, NSImageSymbolConfiguration,
};
use objc2_core_foundation::{CFRetained, CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapContextCreateImage, CGBlendMode, CGColorSpace, CGContext,
    CGImage, CGImageAlphaInfo, CGImageByteOrderInfo, kCGColorSpaceSRGB,
};
use objc2_foundation::NSString;

/// The point size a symbol is measured at: its size grows with its point
/// size, so this one gives the point size that fits a view.
const MEASURE: CGFloat = 100.0;

/// `symbol` drawn at `scale` pixels per point with its longer side as many
/// pixels as `longest` asks for its size in points at the system's default
/// point size (which it returns too, as the image's natural size; the
/// caller bounds the side, as [`nsh_core::image::Want::longest`] does). An
/// error names a symbol the system does not have, or a system before
/// macOS 11.
pub(super) fn draw(
    symbol: &Symbol,
    scale: f32,
    longest: impl FnOnce(Size<Points>) -> u32,
) -> Result<(CFRetained<CGImage>, Size<Points>), String> {
    if !NSImage::class()
        .metaclass()
        .responds_to(sel!(imageWithSystemSymbolName:accessibilityDescription:))
    {
        return Err("SF Symbols need macOS 11 or later".into());
    }
    let missing = || format!("no SF Symbol named `{}`", symbol.name);
    let name = NSString::from_str(&symbol.name);
    let base = NSImage::imageWithSystemSymbolName_accessibilityDescription(&name, None)
        .ok_or_else(missing)?;
    let own = base.size();
    let natural = size(Points(own.width as f32), Points(own.height as f32));
    let weight = weight(symbol);
    let at = |points: CGFloat| {
        let config = NSImageSymbolConfiguration::configurationWithPointSize_weight(points, weight);
        base.imageWithSymbolConfiguration(&config)
    };
    let scale = CGFloat::from(scale.max(0.01));
    let measured = at(MEASURE).ok_or_else(missing)?.size();
    let widest = measured.width.max(measured.height);
    if widest <= 0.0 {
        return Err(missing());
    }
    let n = longest(natural).max(1);
    let fit = n as CGFloat / scale / widest;
    let picture = at(MEASURE * fit).ok_or_else(missing)?;
    let drawn = CGSize::new(measured.width * fit, measured.height * fit);
    let image = render(&picture, drawn, scale).ok_or("cannot draw the SF Symbol")?;
    Ok((image, natural))
}

/// The AppKit weight nearest the symbol's (regular without one).
fn weight(symbol: &Symbol) -> NSFontWeight {
    // SAFETY: immutable AppKit constants.
    let weights = unsafe {
        [
            NSFontWeightUltraLight,
            NSFontWeightThin,
            NSFontWeightLight,
            NSFontWeightRegular,
            NSFontWeightMedium,
            NSFontWeightSemibold,
            NSFontWeightBold,
            NSFontWeightHeavy,
            NSFontWeightBlack,
        ]
    };
    symbol
        .weight_step()
        .map_or(weights[3], |step| weights[usize::from(step) - 1])
}

/// `image` drawn into `size` points at `scale`, then made black where it
/// drew (AppKit draws a symbol in its own color).
fn render(image: &NSImage, size: CGSize, scale: CGFloat) -> Option<CFRetained<CGImage>> {
    let width = (size.width * scale).ceil().max(1.0) as usize;
    let height = (size.height * scale).ceil().max(1.0) as usize;
    // The whole bitmap, in points: a fraction of a pixel wider than `size`.
    let rect = CGRect::new(
        CGPoint::new(0.0, 0.0),
        CGSize::new(width as CGFloat / scale, height as CGFloat / scale),
    );
    // SAFETY: a constant of CoreGraphics; CoreGraphics allocates the pixels
    // (null data, row bytes of its choice) of an RGBA context it owns.
    // AppKit draws into it as the main thread's current context (the
    // caller's thread), and the previous one is current again before the
    // context is read.
    unsafe {
        let space = CGColorSpace::with_name(Some(kCGColorSpaceSRGB))?;
        let context = CGBitmapContextCreate(
            std::ptr::null_mut(),
            width,
            height,
            8,
            0,
            Some(&space),
            CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0,
        )?;
        // AppKit draws y up, in points.
        CGContext::scale_ctm(Some(&context), scale, scale);
        let previous = NSGraphicsContext::currentContext();
        let graphics = NSGraphicsContext::graphicsContextWithCGContext_flipped(&context, false);
        NSGraphicsContext::setCurrentContext(Some(&graphics));
        image.drawInRect(rect);
        NSGraphicsContext::setCurrentContext(previous.as_deref());
        CGContext::set_blend_mode(Some(&context), CGBlendMode::SourceIn);
        CGContext::set_rgb_fill_color(Some(&context), 0.0, 0.0, 0.0, 1.0);
        CGContext::fill_rect(Some(&context), rect);
        CGBitmapContextCreateImage(Some(&context))
    }
}

#[cfg(test)]
mod tests {
    use nsh_core::element::FontWeight;

    use super::*;

    /// The alpha of pixel (`x`, `y`) of `image` and whether it is black.
    fn pixel(image: &CGImage, x: usize, y: usize) -> (u8, bool) {
        let (w, h) = (CGImage::width(Some(image)), CGImage::height(Some(image)));
        let mut out = vec![0u8; w * h * 4];
        // SAFETY: `out` holds `w * 4 * h` bytes and outlives the context.
        unsafe {
            let space = CGColorSpace::with_name(Some(kCGColorSpaceSRGB)).unwrap();
            let context = CGBitmapContextCreate(
                out.as_mut_ptr().cast(),
                w,
                h,
                8,
                w * 4,
                Some(&space),
                CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0,
            )
            .unwrap();
            CGContext::draw_image(
                Some(&context),
                CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w as _, h as _)),
                Some(image),
            );
        }
        let p = &out[(y * w + x) * 4..][..4];
        (p[3], p[0] == 0 && p[1] == 0 && p[2] == 0)
    }

    #[test]
    fn draws_at_the_size_asked_in_black() {
        let symbol = Symbol::new("circle.fill");
        let (image, natural) = draw(&symbol, 2.0, |_| 48).unwrap();
        assert!(natural.width.0 > 0.0 && natural.height.0 > 0.0);
        let (w, h) = (CGImage::width(Some(&image)), CGImage::height(Some(&image)));
        assert_eq!(w.max(h), 48, "{w}×{h}");
        // Opaque black in the middle of the circle, clear in a corner.
        assert_eq!(pixel(&image, w / 2, h / 2), (255, true));
        assert_eq!(pixel(&image, 0, 0).0, 0);
        // The natural size goes to `longest`.
        let mut seen = None;
        draw(&symbol, 1.0, |n| {
            seen = Some(n);
            10
        })
        .unwrap();
        assert_eq!(seen, Some(natural));
    }

    #[test]
    fn weights_change_the_strokes() {
        let ink = |weight| {
            let symbol = Symbol::new("circle").weight(weight);
            let (image, _) = draw(&symbol, 1.0, |_| 64).unwrap();
            let (w, h) = (CGImage::width(Some(&image)), CGImage::height(Some(&image)));
            (0..h)
                .flat_map(|y| (0..w).map(move |x| (x, y)))
                .map(|(x, y)| u32::from(pixel(&image, x, y).0))
                .sum::<u32>()
        };
        assert!(ink(FontWeight::BLACK) > ink(FontWeight::THIN));
    }

    #[test]
    fn unknown_names_fail() {
        let error = draw(&Symbol::new("no.such.symbol.nsh"), 2.0, |_| 10).unwrap_err();
        assert!(error.contains("no.such.symbol.nsh"), "{error}");
    }
}
