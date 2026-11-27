# md-editor changes to winit

This directory is not part of upstream winit. `vendor/winit` is winit **v0.30.13**
(upstream commit `e9809ef54b18499bb4f2cac945719ecc2a61061b`, the exact version iced 0.14
resolves to), vendored into md-editor and swapped in with `[patch.crates-io]` in the
workspace `Cargo.toml`. Only the `.github/` directory was left out.

| Patch | What it does |
| :--- | :--- |
| `0001-wayland-tablet-input.patch` | Wayland graphics-tablet input (`zwp_tablet_v2`): adds `src/platform_impl/linux/wayland/seat/tablet.rs`, binds the tablet manager per seat, and exposes `winit::platform::wayland::{set_tablet_tool_handler, is_tablet_input_available, TabletToolEvent, TabletToolPhase}`. Tool frames go to the handler first, and unclaimed ones are replayed as ordinary cursor and left-button events, so a pen still works as a mouse. |

## Upgrading winit

1. Vendor the release iced resolves to (`cargo tree -i winit`), leaving out `.github/`.
2. Reapply each patch: `git apply --directory=vendor/winit vendor/winit/md-editor-patches/*.patch`.
3. Build, then run `cargo test -p md-editor-native pen_wayland` and try a pen in the ink
   window (the log says `Pen input ready`).

Changing winit here? Update the matching patch too, so this directory stays the complete
record of how `vendor/winit` differs from upstream.
