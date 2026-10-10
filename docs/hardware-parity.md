# Hardware parity

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** major (created from a source audit) · **Target:** Microsoft Excel (Microsoft 365), Excel for Mac 16.113.4

The hardware Excel uses, per platform, against GridCraft. Part of
[target-app-parity.md](target-app-parity.md). **Hardware: ~40% ready, 25–40 h** (estimated).

| Hardware feature | Excel | GridCraft (macOS · Windows · Linux · BSD · web) | Gap and estimate |
|---|---|---|---|
| Multi-core recalculation | Multi-threaded recalculation on all cores (Windows and Mac), configurable thread count | Single-threaded everywhere (no threading in `crates/calc`) | Parallel recalc of independent dependency chains on native builds; WASM stays single-threaded or uses workers later: 12–18 h |
| GPU rendering of the grid and charts | DirectX / Metal accelerated drawing | wgpu: Metal · DirectX 12 (default since 10-08) · Vulkan/GL · Vulkan/GL · WebGPU or WebGL2 | At parity in kind. Driver problems: Intel UHD crash on Windows (#120), screen flashing on Windows 11 (#76): 3–6 h |
| Pen and touch (Draw tab) | Pen pressure and tilt, eraser end, touch mode, Ink to Shape, ink replay | Pen, highlighter, eraser, Ink to Shape with mouse/pen input; **no pressure or tilt**, no touch mode | Pressure-sensitive strokes where winit reports force; touch-friendly ribbon spacing: 4–6 h |
| Trackpad and mouse | Smooth scrolling, pinch zoom, horizontal scroll, Magic Mouse | Smooth scrolling and zoom via egui | Verify pinch zoom on each platform: 1–2 h |
| Multiple windows and monitors | One window per workbook, New Window for the same workbook, per-monitor DPI | One window; multiple workbooks switch inside it (#162; PR #171) | Native window per workbook, New Window, Arrange All across monitors: 6–8 h |
| High-DPI and display scaling | yes | yes (display scale matched to desktop density) | — |
| Accessibility hardware (screen readers, braille displays, switch control) | Narrator, JAWS, NVDA, VoiceOver | AccessKit enabled in eframe; grid cells and ribbon not yet exposed as an accessible tree | Counted under UI ([ui-parity.md](ui-parity.md)) |
| Printers | System print with driver options | Desktop prints through the system dialog via PDF; web exports only | Counted under print |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | major | Created: single-threaded calc, wgpu rendering, no pen pressure, single window |
