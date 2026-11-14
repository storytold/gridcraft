//! GridCraft owns the settings and file format. Only the renderer module calls PhotoCraft.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod png;
mod renderer;

pub use png::{encode, has_source, source};
pub use renderer::{LiveBrush, render};
use serde::{Deserialize, Serialize};

pub const MAX_SAMPLES: usize = 4096;
pub const PIXELS_PER_UNIT: f64 = 2.0;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Preset {
    #[default]
    HardRound,
    SoftRound,
    Chalk,
    Highlighter,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Style {
    pub preset: Preset,
    pub color: [u8; 3],
    pub size: f32,
    pub hardness: f32,
    pub opacity: f32,
    pub flow: f32,
    pub smoothing: f32,
    pub pressure_size: bool,
    pub seed: u32,
}

impl Default for Style {
    fn default() -> Self {
        Self::for_preset(Preset::HardRound)
    }
}

impl Style {
    pub fn for_preset(preset: Preset) -> Self {
        let (size, hardness, opacity, flow, color) = match preset {
            Preset::HardRound => (6.0, 1.0, 1.0, 1.0, [31, 95, 201]),
            Preset::SoftRound => (18.0, 0.0, 1.0, 0.35, [31, 95, 201]),
            Preset::Chalk => (14.0, 0.7, 0.9, 0.7, [31, 95, 201]),
            Preset::Highlighter => (16.0, 1.0, 0.35, 1.0, [242, 227, 58]),
        };
        Self { preset, color, size, hardness, opacity, flow, smoothing: 0.1, pressure_size: preset != Preset::Highlighter, seed: 1 }
    }

    pub fn validate(self) -> Result<Self, Error> {
        if !(1.0..=64.0).contains(&self.size)
            || ![self.hardness, self.opacity, self.flow, self.smoothing].into_iter().all(|v| (0.0..=1.0).contains(&v))
        {
            return Err(Error::Style);
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Sample {
    pub x: f64,
    pub y: f64,
    pub pressure: f32,
    pub tilt_x: f32,
    pub tilt_y: f32,
    pub time_ms: f64,
}

impl Default for Sample {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0, pressure: 1.0, tilt_x: 0.0, tilt_y: 0.0, time_ms: 0.0 }
    }
}

impl Sample {
    fn valid(self) -> bool {
        [self.x, self.y].into_iter().all(|v| v.abs() <= 1_000_000_000.0)
            && (0.0..=1.0).contains(&self.pressure)
            && (-90.0..=90.0).contains(&self.tilt_x)
            && (-90.0..=90.0).contains(&self.tilt_y)
            && (0.0..=1.0e15).contains(&self.time_ms)
    }
}

/// The output's offset and dimensions are in raster pixels relative to the first sample.
pub struct Raster {
    pub offset: [i32; 2],
    pub size: [u32; 2],
    pub rgba: Vec<u8>,
    pub pixels_per_unit: f64,
}

impl Raster {
    pub fn mirror_x(&mut self) {
        let width = self.size[0] as usize;
        for row in self.rgba.chunks_exact_mut(width * 4) {
            for x in 0..width / 2 {
                for channel in 0..4 {
                    row.swap(x * 4 + channel, (width - 1 - x) * 4 + channel);
                }
            }
        }
    }

    /// Crop to the part inside the worksheet, keeping the coordinate of the first pixel.
    pub fn crop_origin(&mut self, origin: [f64; 2], units_per_pixel: [f64; 2]) -> Result<(), Error> {
        let [width, height] = self.size.map(|v| v as usize);
        let skip = |axis: usize, extent: usize| {
            ((-origin[axis] / units_per_pixel[axis]).ceil() - f64::from(self.offset[axis])).clamp(0.0, extent as f64) as usize
        };
        let (left, top) = (skip(0, width), skip(1, height));
        if left == width || top == height {
            return Err(Error::Empty);
        }
        if left == 0 && top == 0 {
            return Ok(());
        }
        let mut cropped = Vec::with_capacity((width - left) * (height - top) * 4);
        for row in self.rgba.chunks_exact(width * 4).skip(top) {
            cropped.extend_from_slice(&row[left * 4..]);
        }
        self.offset[0] += left as i32;
        self.offset[1] += top as i32;
        self.size = [(width - left) as u32, (height - top) as u32];
        self.rgba = cropped;
        Ok(())
    }
}

/// Stored in a versioned private chunk in the PNG. Coordinates are relative to the first sample.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub version: u8,
    pub style: Style,
    pub samples: Vec<Sample>,
    pub offset: [i32; 2],
    pub size: [u32; 2],
    #[serde(default)]
    pub mirrored: bool,
    // Existing strokes used this resolution implicitly. Keep their serialized form readable
    // by earlier versions and write the scale only for adaptively rendered strokes.
    #[serde(default = "default_resolution", skip_serializing_if = "default_pixels_per_unit")]
    pub pixels_per_unit: f64,
}

fn default_resolution() -> f64 {
    PIXELS_PER_UNIT
}

fn default_pixels_per_unit(resolution: &f64) -> bool {
    *resolution == PIXELS_PER_UNIT
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Brush settings are outside the supported range")]
    Style,
    #[error("Brush samples must contain finite coordinates, pressure, tilt and time within their supported ranges")]
    Sample,
    #[error("This brush stroke exceeds the size or work limit. Split it into shorter strokes")]
    Limit,
    #[error("The brush produced no visible stroke")]
    Empty,
    #[error("The brush renderer failed")]
    Render,
    #[error("The brush picture could not be encoded")]
    Encode,
}
