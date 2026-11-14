//! How a picture sits in its cell (IMAGE's sizing argument).

use serde::{Deserialize, Serialize};

/// Largest custom height or width, in pixels.
pub const MAX_PICTURE_PIXELS: i64 = 16_384;

/// How a picture is sized in its cell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PictureSizing {
    /// Fits inside the cell, keeping its aspect ratio (IMAGE sizing 0).
    #[default]
    Fit,
    /// Stretches to cover the cell, ignoring the aspect ratio (IMAGE sizing 1).
    Fill,
    /// Shown at its own pixel size from the cell's top-left; may run past the cell (IMAGE sizing 2).
    Original,
    /// Shown at the given size in pixels from the cell's top-left (IMAGE sizing 3).
    Custom { width: u32, height: u32 },
}

impl PictureSizing {
    /// The sizing IMAGE's `sizing` code gives, with its optional `height` and `width` in pixels.
    /// `None` when the combination is one Excel rejects: a code outside 0–3, height or width given
    /// with codes 0–2, or a custom size missing either dimension or outside 1–16,384.
    pub fn from_code(code: i64, height: Option<i64>, width: Option<i64>) -> Option<Self> {
        match code {
            0..=2 if height.is_none() && width.is_none() => Some(match code {
                0 => PictureSizing::Fit,
                1 => PictureSizing::Fill,
                _ => PictureSizing::Original,
            }),
            3 => Some(PictureSizing::Custom { width: pixels(width?)?, height: pixels(height?)? }),
            _ => None,
        }
    }

    /// Whether this is the default, which files and the model leave out.
    pub fn is_default(&self) -> bool {
        *self == PictureSizing::Fit
    }
}

fn pixels(value: i64) -> Option<u32> {
    (1..=MAX_PICTURE_PIXELS).contains(&value).then_some(value as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_map_to_sizing_and_reject_invalid_combinations() {
        assert_eq!(PictureSizing::from_code(0, None, None), Some(PictureSizing::Fit));
        assert_eq!(PictureSizing::from_code(1, None, None), Some(PictureSizing::Fill));
        assert_eq!(PictureSizing::from_code(2, None, None), Some(PictureSizing::Original));
        assert_eq!(PictureSizing::from_code(3, Some(40), Some(60)), Some(PictureSizing::Custom { width: 60, height: 40 }));
        assert_eq!(PictureSizing::from_code(0, Some(40), None), None);
        assert_eq!(PictureSizing::from_code(2, None, Some(60)), None);
        assert_eq!(PictureSizing::from_code(3, Some(40), None), None);
        assert_eq!(PictureSizing::from_code(3, Some(0), Some(60)), None);
        assert_eq!(PictureSizing::from_code(3, Some(40), Some(MAX_PICTURE_PIXELS + 1)), None);
        assert_eq!(PictureSizing::from_code(4, None, None), None);
        assert_eq!(PictureSizing::from_code(-1, None, None), None);
    }
}
