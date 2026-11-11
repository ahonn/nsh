//! SF Symbols as images ([`nsh_core::image::Symbol`], `symbol:<name>`):
//! drawn by UIKit on the main thread when a view asks, never loaded. The
//! symbol is drawn in black with its own alpha, so the view's `tintColor`
//! (a mask of the image, [`crate::decor`]) colors it as any template
//! image, and follows the appearance as the tint does.

use nsh_core::image::Symbol;
use objc2_core_foundation::{CFRetained, CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapContextCreateImage, CGBlendMode, CGColorSpace, CGContext,
    CGImage, CGImageAlphaInfo, CGImageByteOrderInfo, kCGColorSpaceSRGB,
};
use objc2_foundation::NSString;
use objc2_ui_kit::{
    UIGraphicsPopContext, UIGraphicsPushContext, UIImage, UIImageSymbolConfiguration,
    UIImageSymbolWeight,
};

/// The point size a symbol is measured at: its size grows with its point
/// size, so this one gives the point size that fits a view.
const MEASURE: CGFloat = 100.0;

/// The longest side drawn, in pixels: larger views scale the drawing.
const MAX_SIDE: u32 = 4096;

/// A symbol drawn for a view.
pub(super) struct Drawn {
    /// Black, through the symbol's alpha.
    pub(super) image: CFRetained<CGImage>,
    /// Its size in whole points at the system's default point size, as
    /// `onLoad` reports it.
    pub(super) natural: (u32, u32),
}

/// `symbol` drawn with its longer side `max_pixels` pixels (0: at its
/// default point size), at `scale` pixels per point: a view that contains
/// it (a symbol's default `objectFit`) in a square box shows every pixel
/// as drawn. An error names a symbol the system does not have.
pub(super) fn draw(symbol: &Symbol, max_pixels: u32, scale: f64) -> Result<Drawn, String> {
    let name = NSString::from_str(&symbol.name);
    let weight = UIImageSymbolWeight(symbol.weight_step().map_or(0, isize::from));
    let image = |points: Option<CGFloat>| {
        let config = match points {
            Some(points) => {
                UIImageSymbolConfiguration::configurationWithPointSize_weight(points, weight)
            }
            None => UIImageSymbolConfiguration::configurationWithWeight(weight),
        };
        UIImage::systemImageNamed_withConfiguration(&name, Some(&config))
    };
    let missing = || format!("no SF Symbol named `{}`", symbol.name);
    let natural = image(None).ok_or_else(missing)?;
    // SAFETY: a plain getter on a valid image.
    let own = unsafe { natural.size() };
    let (picture, size) = match max_pixels.min(MAX_SIDE) {
        0 => (natural, own),
        n => {
            // SAFETY: a plain getter on a valid image.
            let measured = unsafe { image(Some(MEASURE)).ok_or_else(missing)?.size() };
            let longest = measured.width.max(measured.height);
            if longest <= 0.0 {
                return Err(missing());
            }
            let fit = n as CGFloat / scale / longest;
            let size = CGSize::new(measured.width * fit, measured.height * fit);
            (image(Some(MEASURE * fit)).ok_or_else(missing)?, size)
        }
    };
    let image = render(&picture, size, scale).ok_or("cannot draw the SF Symbol")?;
    let round = |v: CGFloat| v.round().max(1.0) as u32;
    Ok(Drawn {
        image,
        natural: (round(own.width), round(own.height)),
    })
}

/// `image` drawn into `size` points at `scale`, then made black where it
/// drew (UIKit draws a symbol in its own color).
fn render(image: &UIImage, size: CGSize, scale: f64) -> Option<CFRetained<CGImage>> {
    let width = (size.width * scale).ceil().max(1.0) as usize;
    let height = (size.height * scale).ceil().max(1.0) as usize;
    // The whole bitmap, in points: a fraction of a pixel wider than `size`.
    let rect = CGRect::new(
        CGPoint::new(0.0, 0.0),
        CGSize::new(width as CGFloat / scale, height as CGFloat / scale),
    );
    // SAFETY: a constant of CoreGraphics; CoreGraphics allocates the pixels
    // (null data, row bytes of its choice) of an RGBA context it owns. UIKit
    // draws into a pushed context on the main thread (the caller's), which
    // is popped before the context is read.
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
        // UIKit draws y down, in points.
        CGContext::translate_ctm(Some(&context), 0.0, height as CGFloat);
        CGContext::scale_ctm(Some(&context), scale, -scale);
        UIGraphicsPushContext(&context);
        image.drawInRect(rect);
        UIGraphicsPopContext();
        CGContext::set_blend_mode(Some(&context), CGBlendMode::SourceIn);
        CGContext::set_rgb_fill_color(Some(&context), 0.0, 0.0, 0.0, 1.0);
        CGContext::fill_rect(Some(&context), rect);
        CGBitmapContextCreateImage(Some(&context))
    }
}
