use std::panic::{AssertUnwindSafe, catch_unwind};

use photocraft_color::PixelFormat;
use photocraft_geom::Rect;
use photocraft_paint::brush::Smoothing;
use photocraft_paint::{BrushSettings, Pattern, PatternStyle, StrokePoint, StrokeRenderer, Texture};
use photocraft_raster::Surface;

use crate::{Error, MAX_SAMPLES, PIXELS_PER_UNIT, Preset, Raster, Sample, Style};

const MAX_SIDE: u32 = 2048;
const MAX_PIXELS: u64 = 1_500_000;
const MAX_WORK: f64 = 28_000_000.0;
const RESOLUTIONS: [f64; 4] = [PIXELS_PER_UNIT, 1.0, 0.5, 0.25];

pub(crate) fn valid_resolution(resolution: f64) -> bool {
    RESOLUTIONS.contains(&resolution)
}

fn settings(style: Style, resolution: f64) -> BrushSettings {
    let [r, g, b] = style.color.map(|v| f32::from(v) / 255.0);
    let texture = if style.preset == Preset::Chalk {
        Texture {
            enabled: true,
            pattern: Pattern::Procedural { style: PatternStyle::Paper, size: 64, seed: style.seed },
            depth: 0.95,
            ..Texture::default()
        }
    } else {
        Texture::default()
    };
    BrushSettings {
        size: style.size * resolution as f32,
        hardness: style.hardness,
        spacing: 0.15,
        opacity: style.opacity,
        flow: style.flow,
        color: [r, g, b, 1.0],
        pressure_size: style.pressure_size,
        roundness: if style.preset == Preset::Highlighter { 0.55 } else { 1.0 },
        angle: if style.preset == Preset::Highlighter { 20.0 } else { 0.0 },
        smoothing: Smoothing { amount: style.smoothing, catch_up: false, catch_up_on_end: true, adjust_for_zoom: false, ..Smoothing::default() },
        texture,
        noise: style.preset == Preset::Chalk,
        seed: u64::from(style.seed),
        ..BrushSettings::default()
    }
}

/// Guards calls into the pinned renderer. On an error, the caller discards the gesture.
fn guarded<T>(f: impl FnOnce() -> T) -> Result<T, Error> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|_| Error::Render)
}

/// Preview stays at the highest resolution the entire gesture can fit. It only replays earlier
/// samples when crossing a resolution threshold, so the number of replays per stroke is bounded.
pub struct LiveBrush {
    style: Style,
    samples: Vec<Sample>,
    budget: Budget,
    rasterizer: Rasterizer,
}

impl LiveBrush {
    pub fn new(style: Style, first: Sample) -> Result<Self, Error> {
        let style = style.validate()?;
        let budget = Budget::new(first)?;
        let mut rasterizer = Rasterizer::new(style, first, budget.resolution(style)?)?;
        rasterizer.push(first)?;
        Ok(Self { style, samples: vec![first], budget, rasterizer })
    }

    pub fn push(&mut self, sample: Sample) -> Result<(), Error> {
        let budget = self.budget.with_sample(sample)?;
        let resolution = budget.resolution(self.style)?;
        if resolution == self.rasterizer.resolution {
            self.rasterizer.push(sample)?;
        } else {
            let mut rasterizer = Rasterizer::new(self.style, budget.first, resolution)?;
            for &point in self.samples.iter().chain(std::iter::once(&sample)) {
                rasterizer.push(point)?;
            }
            self.rasterizer = rasterizer;
        }
        self.budget = budget;
        self.samples.push(sample);
        Ok(())
    }

    pub fn samples(&self) -> &[Sample] {
        &self.samples
    }

    pub fn into_samples(self) -> Vec<Sample> {
        self.samples
    }

    pub fn preview(&mut self) -> Result<Raster, Error> {
        self.rasterizer.preview()
    }
}

#[derive(Clone, Copy)]
struct Budget {
    first: Sample,
    last: Sample,
    min: [f64; 2],
    max: [f64; 2],
    distance: f64,
    count: usize,
}

impl Budget {
    fn new(first: Sample) -> Result<Self, Error> {
        if !first.valid() {
            return Err(Error::Sample);
        }
        Ok(Self { first, last: first, min: [0.0; 2], max: [0.0; 2], distance: 0.0, count: 1 })
    }

    fn with_sample(mut self, sample: Sample) -> Result<Self, Error> {
        if !sample.valid() || sample.time_ms < self.last.time_ms {
            return Err(Error::Sample);
        }
        if self.count >= MAX_SAMPLES {
            return Err(Error::Limit);
        }
        let [x, y] = [sample.x - self.first.x, sample.y - self.first.y];
        self.min = [self.min[0].min(x), self.min[1].min(y)];
        self.max = [self.max[0].max(x), self.max[1].max(y)];
        self.distance += (sample.x - self.last.x).hypot(sample.y - self.last.y);
        self.last = sample;
        self.count += 1;
        Ok(self)
    }

    fn resolution(self, style: Style) -> Result<f64, Error> {
        RESOLUTIONS.into_iter().find(|&resolution| self.fits(style, resolution)).ok_or(Error::Limit)
    }

    fn fits(self, style: Style, resolution: f64) -> bool {
        // Padding conservatively covers the round, rotated and soft tips before any pixels are
        // allocated. The work cap also rejects repeated scribbling in a small box.
        let margin = (f64::from(style.size) * resolution * 2.0).ceil() + 4.0;
        let extent = |axis: usize| (self.max[axis] * resolution).floor() - (self.min[axis] * resolution).floor() + 2.0 + 2.0 * margin;
        let (width, height) = (extent(0), extent(1));
        width.max(height) <= f64::from(MAX_SIDE)
            && width * height <= MAX_PIXELS as f64
            && (self.distance * resolution + self.count as f64) * margin * 16.0 <= MAX_WORK
    }
}

/// The PhotoCraft renderer paints over the same transparent surface on every preview.
struct Rasterizer {
    renderer: StrokeRenderer,
    transparent: Surface,
    target: Surface,
    tail: Rect,
    origin: Sample,
    resolution: f64,
}

impl Rasterizer {
    fn new(style: Style, origin: Sample, resolution: f64) -> Result<Self, Error> {
        guarded(|| {
            let brush = settings(style, resolution);
            Self {
                renderer: StrokeRenderer::new(&brush, Some(PixelFormat::RGBA8), 1.0),
                transparent: Surface::new(PixelFormat::RGBA8),
                target: Surface::new(PixelFormat::RGBA8),
                tail: Rect::EMPTY,
                origin,
                resolution,
            }
        })
    }

    fn push(&mut self, sample: Sample) -> Result<(), Error> {
        let point = StrokePoint {
            x: (sample.x - self.origin.x) * self.resolution,
            y: (sample.y - self.origin.y) * self.resolution,
            pressure: sample.pressure,
            tilt_x: sample.tilt_x,
            tilt_y: sample.tilt_y,
            time: sample.time_ms,
            ..StrokePoint::default()
        };
        guarded(|| self.renderer.push(&[point]))
    }

    fn preview(&mut self) -> Result<Raster, Error> {
        guarded(|| {
            let (_, tail) = self.renderer.composite_live(self.tail, &self.transparent, &mut self.target, None, false);
            self.tail = tail;
            self.frame(self.renderer.bounds().union(&tail))
        })?
    }

    fn finish(mut self) -> Result<Raster, Error> {
        guarded(|| {
            self.renderer.finish();
            // Compose from transparent pixels. The interactive tail is derived data only.
            self.target = Surface::new(PixelFormat::RGBA8);
            let bounds = self.renderer.composite(&self.transparent, &mut self.target, None, false, true);
            self.frame(bounds)
        })?
    }

    fn frame(&self, bounds: Rect) -> Result<Raster, Error> {
        if bounds.is_empty() {
            return Err(Error::Empty);
        }
        check_bounds(bounds)?;
        let rgba = self.target.try_to_interleaved(bounds).map_err(|_| Error::Render)?;
        Ok(Raster { offset: [bounds.x0, bounds.y0], size: [bounds.width(), bounds.height()], rgba, pixels_per_unit: self.resolution })
    }
}

fn check_bounds(bounds: Rect) -> Result<(), Error> {
    if bounds.width() > MAX_SIDE || bounds.height() > MAX_SIDE || u64::from(bounds.width()) * u64::from(bounds.height()) > MAX_PIXELS {
        return Err(Error::Limit);
    }
    Ok(())
}

pub fn render(style: Style, samples: &[Sample]) -> Result<Raster, Error> {
    let (first, rest) = samples.split_first().ok_or(Error::Empty)?;
    if samples.len() > MAX_SAMPLES {
        return Err(Error::Limit);
    }
    let style = style.validate()?;
    let budget = rest.iter().try_fold(Budget::new(*first)?, |budget, &sample| budget.with_sample(sample))?;
    let mut rasterizer = Rasterizer::new(style, *first, budget.resolution(style)?)?;
    for &sample in samples {
        rasterizer.push(sample)?;
    }
    let raster = rasterizer.finish()?;
    if raster.rgba.as_chunks::<4>().0.iter().all(|px| px[3] == 0) {
        return Err(Error::Empty);
    }
    Ok(raster)
}
