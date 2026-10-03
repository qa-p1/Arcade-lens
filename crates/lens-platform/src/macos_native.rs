//! macOS cursor position. Window control on macOS requires the
//! Accessibility API and user consent; it is reported as unsupported.

use lens_core::geometry::Point;
use objc2_core_graphics::{CGEvent, CGMainDisplayID};

pub fn cursor() -> Option<Point> {
    // CGEventGetLocation is in global display points with a top-left origin.
    let ev = CGEvent::new(None)?;
    let p = CGEvent::location(Some(&ev));
    let scale = {
        let id = CGMainDisplayID();
        let px = objc2_core_graphics::CGDisplayPixelsWide(id) as f64;
        let bounds = objc2_core_graphics::CGDisplayBounds(id);
        if bounds.size.width > 0.0 {
            px / bounds.size.width
        } else {
            1.0
        }
    };
    Some(Point { x: (p.x * scale) as i32, y: (p.y * scale) as i32 })
}
