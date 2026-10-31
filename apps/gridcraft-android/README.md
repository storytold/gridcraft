# GridCraft on Android

GridCraft on Android: a small `apps/` crate that hosts the existing `gridcraft_ui_egui` interface in an
`android-activity` (GameActivity) window, on top of `gridcraft_engine`. The desktop entry crate is not
compiled, so `rfd`, `arboard` and the macOS/Windows/Linux platform code never enter the dependency
graph.

## What the shell does

- **Entry point** — builds as `libmain.so` (`[lib] name = "main"`, `crate-type = ["cdylib"]`), loaded
  with `System.loadLibrary`; `android_main` is the usual `android-activity` entry.
- **Storage** — `AndroidApp::internal_data_path()` is the data directory, and GridCraft's `ui.json`/`prefs.json` pair is loaded before the first frame and written back in `on_exit`.
- **File pickers** — *not wired yet*: `Services` is left at its default, so File ▸ Open and Save are inert.
  The shell exists to prove the target builds and the desktop UI comes up; the SAF
  bridge is the next change.
- **Fonts** — Android does not expose the Unix font directories, so the CJK face from `/system/fonts`
  is appended to every family *after* GridCraft installs its theme (any earlier, and the theme's named
  families would be dropped).
- **Touch** — hit targets are raised to 40 pt, sliders widened and hover tooltips suppressed, applied
  after the theme's style exists. The layout is not zoomed: it is designed for a desktop window and a
  phone gives ~411 pt, so the next step is a touch rework, not a scale factor.

## Known gaps

- No file pickers yet (see above); the app starts on a blank workbook.
- Printing has no backend.

- The system clipboard is not wired (`arboard` has no Android backend); GridCraft's own clipboard is
  still used.
- The launcher icons are the Android Studio template's placeholder art.

## Build

```bash
cargo ndk -t arm64-v8a -o apps/gridcraft-android/jniLibs build --release -p gridcraft-android
cd apps/gridcraft-android/android && ./gradlew assembleRelease
```

`jniLibs/` and the Gradle output are gitignored; the APK is not part of this change.
