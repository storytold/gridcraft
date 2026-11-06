//! The OS cursor in physical pixels.
//!
//! egui reports the pointer per viewport: a drag that leaves its window keeps no position in it
//! and has none in the window it entered. Moving a tab from one native window to another needs the
//! system cursor, which only the platform can answer — hence this crate, kept out of the
//! `unsafe_code = "forbid"` lint the rest of the workspace uses and deliberately tiny.

/// The cursor in physical pixels, or `None` when the platform cannot say.
#[cfg(windows)]
pub fn cursor_px() -> Option<(f32, f32)> {
    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }
    unsafe extern "system" {
        fn GetCursorPos(point: *mut Point) -> i32;
    }
    let mut point = Point { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut point) } == 0 { None } else { Some((point.x as f32, point.y as f32)) }
}

/// Not implemented off Windows: callers fall back to their own pointer position.
#[cfg(not(windows))]
pub fn cursor_px() -> Option<(f32, f32)> {
    None
}
