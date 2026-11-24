# ARZ-Excalibur v1.0.0

**Native Rust Spotlight Command Bar, 60-FPS Real Timeline Frame Preview Grid, `.prfpset` XML Compiler & Macro Engine for Adobe Premiere Pro (including Premiere Pro 2027)**

Built with **ARZ Studio's** signature **Noir & Brass (`#110E0A` · `#DB9B3E` · `#F4EEE6`)** visual identity — combining zero-latency native `Rust` execution (`WH_KEYBOARD_LL` + `egui` GPU rendering + `quick-xml`) with an auto-managed Adobe Premiere Pro lifecycle.

---

## Key Features

- **Premiere-Bound Auto Lifecycle (Zero 24/7 Background Bloat):** Starts silently in hidden HUD mode (`--managed-by-premiere`) **only** when Adobe Premiere Pro opens, and terminates automatically the instant Premiere Pro closes.
- **Minimalist Noir & Brass Command Bar (`Alt + Space`):** Sleek centered `64px` obsidian bar (`#110E0A`) featuring the custom **ARZ Excalibur** watermark emblem; expands dynamically into a **3-Column Visual Square Grid** on first keystroke.
- **60-FPS Real Timeline Clip-Frame Previews:** Captures the active playhead frame directly from Premiere Pro (`exportFramePNG`) and renders real-time pixel & motion previews (`Warp Stabilizer` optical tracking, `Gaussian Blur`, `Lumetri Color` split-wipe, `Transform / Scale` zoom, `Crop`, `Transitions`, and live audio waveforms) as you navigate cards with the arrow keys (`←` `→` `↑` `↓`).
- **Inline Relative Calculator (`+`, `-`, `*`, `/`, `%`) + Keyframes:** Modify `Scale`, `Position`, `Rotation`, `Anchor Point`, `Opacity`, and `Volume` across all selected timeline clips at once, or drop keyframes at the playhead.
- **Streaming `.prfpset` Preset Compiler:** Automatically discovers and parses Premiere Pro's `Effect Presets and Custom Items.prfpset` via `quick-xml` with live `notify` hot-reload whenever new presets are saved.
- **Direct Win32 Menu-Bar Execution (`WM_COMMAND`):** Triggers top-menu Premiere commands (`Scale to Frame Size`, `Set to Frame Size`, `Scene Edit Detection`, `Audio Gain`, `Render In to Out`) without moving the cursor or requiring Premiere keyboard shortcuts.
- **Full 2-Tab Premiere Settings & Visual Macro Builder:** Accessible inside Premiere via `Window > Extensions > ARZ-Excalibur Settings` to toggle command visibility, bind direct per-command hotkeys, switch between Grid and Classic List layouts, and construct multi-step **User Macros**.

---

## Quick Installation (Windows x64)

1. Run [`run-arz-excalibur.bat`](./run-arz-excalibur.bat) (or execute `powershell -ExecutionPolicy Bypass -File .\install-premiere-bridge.ps1`).
2. Open **Adobe Premiere Pro** (2020 – 2027).
3. Press **`Alt + Space`** to summon **ARZ-Excalibur**, or open **`Window > Extensions > ARZ-Excalibur Settings`** to customize shortcuts and macros.

---

## Building from Source

```powershell
cargo test
cargo build --release
```
