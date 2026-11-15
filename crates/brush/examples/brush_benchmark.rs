//! CPU timings for a fixed set of pointer gestures. Run with cargo run -p gridcraft-brush --example brush_benchmark.

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::time::Instant;

    use gridcraft_brush::{Error, LiveBrush, Preset, Sample, Style, encode, render};

    struct Case {
        name: &'static str,
        span: [f64; 2],
        points: usize,
        style: Style,
    }

    impl Case {
        fn samples(&self) -> Vec<Sample> {
            (0..self.points)
                .map(|i| {
                    let t = i as f64 / (self.points - 1) as f64;
                    Sample {
                        x: 40.0 + t * self.span[0],
                        y: 50.0 + t * self.span[1] + (t * 6.0 * std::f64::consts::PI).sin() * 15.0,
                        pressure: (0.35 + 0.6 * t) as f32,
                        time_ms: i as f64 * 8.0,
                        ..Sample::default()
                    }
                })
                .collect()
        }

        #[allow(clippy::disallowed_methods)] // This benchmark reads the monotonic clock only off wasm.
        fn pass(&self, samples: &[Sample], updates: &mut Vec<f64>, commits: &mut Vec<f64>) -> Result<usize, Error> {
            let (first, moves) = samples.split_first().ok_or(Error::Empty)?;
            let mut live = LiveBrush::new(self.style, *first)?;
            for point in moves {
                let start = Instant::now();
                live.push(*point)?;
                std::hint::black_box(live.preview()?);
                updates.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            let start = Instant::now();
            let raster = render(self.style, samples)?;
            let bytes = encode(&raster, self.style, samples, false)?.len();
            commits.push(start.elapsed().as_secs_f64() * 1000.0);
            Ok(bytes)
        }
    }

    fn timing(values: &mut [f64]) -> String {
        values.sort_by(f64::total_cmp);
        let p = |pct: f64| values.get((pct * values.len() as f64).ceil() as usize - 1).copied().unwrap_or_default();
        format!("median {:.2} ms, p95 {:.2} ms, max {:.2} ms", p(0.5), p(0.95), values.last().copied().unwrap_or_default())
    }

    pub(super) fn run() {
        let cases = [
            Case { name: "Soft round", span: [320.0, 130.0], points: 121, style: Style::for_preset(Preset::SoftRound) },
            Case { name: "Long hard round", span: [1500.0, 800.0], points: 241, style: Style::default() },
            Case { name: "Long 64-unit chalk", span: [1500.0, 800.0], points: 241, style: Style { size: 64.0, ..Style::for_preset(Preset::Chalk) } },
        ];
        for case in cases {
            let samples = case.samples();
            if let Err(err) = case.pass(&samples, &mut Vec::new(), &mut Vec::new()) {
                println!("{}: {err}", case.name);
                continue;
            }
            let (mut updates, mut commits) = (Vec::new(), Vec::new());
            let result = (0..12).try_fold(0, |_, _| case.pass(&samples, &mut updates, &mut commits));
            match result {
                Ok(bytes) => println!(
                    "{}: {} preview updates ({}); {} commits including PNG ({}); {} bytes",
                    case.name,
                    updates.len(),
                    timing(&mut updates),
                    commits.len(),
                    timing(&mut commits),
                    bytes
                ),
                Err(err) => println!("{}: {err}", case.name),
            }
        }
    }
}

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    native::run();
    #[cfg(target_arch = "wasm32")]
    eprintln!("Run this CPU benchmark on a native target.");
}
