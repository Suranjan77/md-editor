# GUI testing on COSMIC

> ⚠️ **This tooling is specific to this one workstation. It is NOT portable.**
> Dogtail, ydotool, OpenCV, `cosmic-screenshot`, and the live COSMIC/Wayland
> session exist only on this machine. **CI and any other machine cannot run the
> GUI smoke.** Do not gate anything on it, and do not assume `/tmp/.ydotool_socket`
> or a desktop session exists elsewhere. The GUI smoke is a *local-only,
> last-mile* supplement — it confirms the app renders/behaves on a real
> compositor, nothing more.
>
> **The portable test tiers (run everywhere, including CI) are:**
> 1. **Behavior** — the windowless shell harness in `shell/tests/*` drives the
>    real `gui::Shell` with semantic messages
>    (`Shell::new(...); shell.update(Message::TreeFileClicked(...))`,
>    `RunCommand`, `Key`, `PaneCommand`, …). This is the equivalent of a
>    DOM-level UI test and is where behavior coverage belongs.
> 2. **Pixel geometry** — the golden draw-plan snapshot
>    (`shell/tests/editor_draw_plan.rs` vs `fixtures/golden.plan.txt`) asserts
>    the exact `PaintOp` stream. Regenerate with `UPDATE_EXPECT=1 cargo test`.
>
> Reach for the COSMIC tooling below only for things those two tiers cannot
> cover (does it actually launch and paint on a real GPU/compositor).

This workstation has GUI automation tools installed system-wide:

- Dogtail `0.9.11` (`python3-dogtail`) for AT-SPI discovery and named controls.
- OpenCV `4.13.0` (`python3-opencv`) for screenshot analysis and image matching.
- ydotool `1.0.4` for Wayland pointer and keyboard injection.
- `cosmic-screenshot` for full-desktop evidence.

Desktop accessibility is enabled. `ydotool.service` is enabled and active.

### Practical gotchas found while driving the smoke (2026-06-14)

- **iced exposes no AT-SPI tree** — `gui-probe.py tree` finds no app nodes,
  so Dogtail cannot address the editor; you fall back to screenshots + OpenCV +
  blind coordinate clicks for almost everything. (Enabling iced's AccessKit
  feature would let Dogtail drive named *chrome*; the editor canvas would stay
  opaque.)
- **ydotool coordinates are not screen pixels.** `ydotoold` here exposes a
  virtual absolute device of roughly `1333×750`, so a click lands at
  `screen ≈ 1.92 × ydotool`. To click screen `(X, Y)` pass
  `ydotool ≈ (X/1.92, Y/1.92)`. This also makes drag-to-resize/maximize and the
  file-tree row clicks land off-target — keyboard paths (`ctrl+p`, arrows) are
  far more reliable than mouse coordinates.
- Launch the app detached (`setsid … & disown`) so it survives the shell that
  started it; window placement and maximize-via-edge-drag are flaky, so prefer
  cropping the floating window over fighting the window manager.

## Agent constraints

GUI automation must run in the live COSMIC session, outside the restricted
filesystem sandbox. In Codex, request host permission for Dogtail, ydotool, and
`cosmic-screenshot` commands. This workstation's daemon socket is
`/tmp/.ydotool_socket`, owned by desktop user; restricted sandbox access is
not a valid connectivity test.

Dogtail should drive accessible window chrome and named widgets. The iced
editor/PDF canvases expose little or no useful AT-SPI structure, so use
screenshots plus OpenCV for those surfaces. Prefer semantic lookup over fixed
coordinates whenever Dogtail can see a control.

Accessibility changes are most reliable after restarting the app under test.

## Quick checks

From repository root:

```bash
python3 scripts/gui-probe.py check
python3 scripts/gui-probe.py tree
python3 scripts/gui-probe.py tree --app 'MD Editor' --depth 5
```

Run these host-side. `check` verifies packages, accessibility setting,
ydotool socket, daemon state, and visible AT-SPI applications. `tree` prints
roles, names, positions, sizes, and actions for accessible nodes.

Capture current desktop:

```bash
cosmic-screenshot --interactive=false --modal=false --notify=false --save-dir /tmp
```

Then inspect latest `/tmp/Screenshot_*.png` with OpenCV or the agent image
viewer. Keep generated screenshots in `/tmp`; do not add them to repository
unless a plan explicitly asks for committed visual evidence.

## Recommended smoke workflow

1. Build the shell with PDF support.
2. Create isolated vault and config directories under `/tmp`.
3. Launch shell in live COSMIC session with isolated `XDG_CONFIG_HOME`.
4. Use Dogtail tree dump to locate accessible chrome.
5. Use ydotool for canvas interactions Dogtail cannot address.
6. Capture before/after screenshots and inspect with OpenCV.
7. Record only observed results in `docs/SMOKE.md`; never infer manual pass
   from unit tests.

Example launch:

```bash
cargo build -p md-shell --features pdfium
XDG_CONFIG_HOME=/tmp/md-editor-gui-config \
  target/debug/md-editor /tmp/md-editor-gui-vault
```

Before handoff after code changes, still run project-required fmt, clippy, and
workspace test gates. GUI smoke supplements those gates; it does not replace
them.
