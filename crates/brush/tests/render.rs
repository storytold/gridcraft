use gridcraft_brush::{Error, LiveBrush, Preset, Sample, Style, encode, has_source, render, source};

fn samples() -> [Sample; 3] {
    [
        Sample { x: 40.0, y: 50.0, pressure: 0.25, ..Sample::default() },
        Sample { x: 75.0, y: 59.0, pressure: 0.7, time_ms: 10.0, ..Sample::default() },
        Sample { x: 97.0, y: 84.0, pressure: 1.0, time_ms: 20.0, ..Sample::default() },
    ]
}

#[test]
fn live_preview_matches_saved_pixels_across_presets() {
    let samples = samples();
    let mut results = Vec::new();
    for preset in [Preset::HardRound, Preset::SoftRound, Preset::Chalk, Preset::Highlighter] {
        let style = Style::for_preset(preset);
        let mut live = LiveBrush::new(style, samples[0]).unwrap();
        for sample in &samples[1..] {
            assert!(live.preview().unwrap().rgba.as_chunks::<4>().0.iter().any(|p| p[3] > 0));
            live.push(*sample).unwrap();
        }
        let preview = live.preview().unwrap();
        let committed = render(style, &samples).unwrap();
        assert_eq!(preview.offset, committed.offset, "{preset:?}");
        assert_eq!(preview.size, committed.size, "{preset:?}");
        assert_eq!(preview.rgba, committed.rgba, "{preset:?}");
        let png = encode(&committed, style, &samples, false).unwrap();
        assert_eq!(image::load_from_memory(&png).unwrap().into_rgba8().into_raw(), committed.rgba);
        assert_eq!(source(&png).unwrap().style, style);
        results.push(committed.rgba);
    }
    assert!(results.windows(2).all(|p| p[0] != p[1]));
}

#[test]
fn pressure_and_limits_reach_the_adapter() {
    let style = Style::default();
    let a = Sample { pressure: 0.25, ..Sample::default() };
    let b = Sample { pressure: 1.0, ..a };
    assert!(render(style, &[a]).unwrap().size[0] < render(style, &[b]).unwrap().size[0]);
    assert!(matches!(render(style, &[a, Sample { x: 1.0e6, ..a }]), Err(Error::Limit)));
    assert!(matches!(render(style, &[Sample { pressure: f32::NAN, ..a }]), Err(Error::Sample)));
    assert!(matches!(render(Style { size: 65.0, ..style }, &[a]), Err(Error::Style)));
}

#[test]
fn brush_sub_options_change_the_rendered_pixels() {
    let samples = samples();
    let style = Style { size: 24.0, ..Style::default() };
    let original = render(style, &samples).unwrap();
    for (option, changed) in [
        ("color", Style { color: [181, 35, 58], ..style }),
        ("size", Style { size: 48.0, ..style }),
        ("hardness", Style { hardness: 0.1, ..style }),
        ("opacity", Style { opacity: 0.25, ..style }),
        ("flow", Style { flow: 0.15, ..style }),
        ("smoothing", Style { smoothing: 0.9, ..style }),
        ("pressure size", Style { pressure_size: false, ..style }),
    ] {
        let raster = render(changed, &samples).unwrap();
        assert_ne!(raster.rgba, original.rgba, "{option} must affect the brush pixels");
    }
}

#[test]
fn desktop_sized_diagonal_stays_visible_from_preview_to_commit() {
    let style = Style::default();
    let start = Sample { x: 100.0, y: 100.0, ..Sample::default() };
    let end = Sample { x: 1000.0, y: 750.0, time_ms: 20.0, ..start };
    let mut live = LiveBrush::new(style, start).unwrap();
    live.push(end).expect("a desktop-sized diagonal should not hit the brush limit");
    let preview = live.preview().unwrap();
    let committed = render(style, &[start, end]).unwrap();
    assert!(committed.pixels_per_unit < gridcraft_brush::PIXELS_PER_UNIT);
    assert_eq!(preview.pixels_per_unit, committed.pixels_per_unit);
    assert_eq!(preview.offset, committed.offset);
    assert_eq!(preview.size, committed.size);
    assert_eq!(preview.rgba, committed.rgba);
    let png = encode(&committed, style, &[start, end], false).unwrap();
    assert_eq!(source(&png).unwrap().pixels_per_unit, committed.pixels_per_unit);
}

#[test]
fn unrecognised_source_leaves_a_valid_png_fallback() {
    let samples = samples();
    let style = Style::default();
    let mut png = encode(&render(style, &samples).unwrap(), style, &samples, false).unwrap();
    let length = u32::from_be_bytes(png[33..37].try_into().unwrap()) as usize;
    let end = 41 + length;
    let version = png[41..end].windows(11).position(|bytes| bytes == b"\"version\":1").unwrap() + 51;
    png[version] = b'9';
    let crc = crc32fast::hash(&png[37..end]);
    png[end..end + 4].copy_from_slice(&crc.to_be_bytes());
    assert!(has_source(&png));
    assert!(source(&png).is_none());
    assert!(image::load_from_memory(&png).is_ok());
}
