# Hardware parity

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-17 · **Change:** minor (Ink to Shape converts Excel's preset shapes) · **Target:** Microsoft Excel (Microsoft 365), Excel for Mac 16.113.4

The hardware Excel uses, per platform, against GridCraft. Part of
[target-app-parity.md](target-app-parity.md). **Hardware: ~40% ready, 25–40 h** (estimated).

| Hardware feature | Excel | GridCraft (macOS · Windows · Linux · BSD · web) | Gap and estimate |
|---|---|---|---|
| Multi-core recalculation | Multi-threaded recalculation on all cores (Windows and Mac), configurable thread count | Single-threaded everywhere (no threading in `crates/calc`) | Parallel recalc of independent dependency chains on native builds; WASM stays single-threaded or uses workers later: 12–18 h |
| GPU rendering of the grid and charts | DirectX / Metal accelerated drawing | wgpu: Metal · DirectX 12 (default since 10-08; OpenGL when DirectX 12 has no GPU driver or fails to start; Vulkan only on request, so the Intel UHD `igvk64.dll` crash (#120) can't happen; shaders compiled with FXC, so no DXC runtime is needed, #339) · Vulkan/GL · Vulkan/GL · WebGPU or WebGL2 | At parity in kind. Driver problems: screen flashing on Windows 11 (#76): 2–4 h |
| Pen and touch (Draw tab) | Pen pressure and tilt, eraser end, touch mode, Ink to Shape, ink replay | Pen, highlighter, eraser, Ink to Shape as a before-drawing mode that converts Excel's preset shapes with mouse/pen input; **no pressure or tilt**, no touch mode | Pressure-sensitive strokes where winit reports force; touch-friendly ribbon spacing: 4–6 h |
| Trackpad and mouse | Smooth scrolling, pinch zoom, horizontal scroll, Magic Mouse | Smooth scrolling and zoom via egui | Verify pinch zoom on each platform: 1–2 h |
| Multiple windows and monitors | One window per workbook, New Window for the same workbook, per-monitor DPI | One window; multiple workbooks switch inside it (#162; PR #171) | Native window per workbook, New Window, Arrange All across monitors: 6–8 h |
| High-DPI and display scaling | yes | yes (display scale matched to desktop density; View › Interface size 75–200 % on top, #230) | — |
| Accessibility hardware (screen readers, braille displays, switch control) | Narrator, JAWS, NVDA, VoiceOver | AccessKit enabled in eframe; grid cells and ribbon not yet exposed as an accessible tree | Counted under UI ([ui-parity.md](ui-parity.md)) |
| Printers | System print with driver options | Desktop submits through craft-print with printer choice, copies, duplex and grayscale; web browser printing | Counted under print |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-17 | minor | Draw ▸ Ink to Shape is a before-drawing mode that converts each stroke to Excel's preset shapes (rectangles/squares, ellipse/circle, diamond, parallelogram, trapezoid, pentagon, hexagon, triangle/right triangle, block and double arrows, lines) |
| 2026-10-12 | minor | Desktop File > Print now discovers printers and submits PDF jobs through pinned craft-print with printer choice, copies, duplex and grayscale; browser printing and PDF export remain available. Remaining print parity includes page ranges and driver-specific tray/options UI; physical printer fidelity needs hardware validation. |
| 2026-10-11 | trivial | Windows: DirectX 12 shaders compiled with FXC, no DXC runtime needed (#339, #354) |
| 2026-10-11 | trivial | Windows: OpenGL fallback when DirectX 12 has no GPU or fails to start; Vulkan never by default (#120) |
| 2026-10-10 | major | Created: single-threaded calc, wgpu rendering, no pen pressure, single window |
