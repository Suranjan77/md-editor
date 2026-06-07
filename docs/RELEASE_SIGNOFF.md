# Release Signoff

Last updated: 2026-06-07

## Candidate

- Branch: `pdf-improv`
- Host: Fedora Linux 44, x86_64, kernel 7.0.10
- Rust: `rustc 1.96.0`, `cargo 1.96.0`

## Linux Results

- Release build succeeds and produces `target/release/md-editor`.
- `libpdfium.so` is present beside the release executable.
- Release GUI starts with isolated config and remains running through a
  five-second smoke window.
- `--install` creates a quoted desktop `Exec` entry and complete icon set under
  an isolated home directory.
- `--uninstall` removes installed desktop entry and icons.
- Portable bundle with `portable.flag` creates
  `md_editor_settings.sqlite` beside the executable and does not create a
  platform-config database.
- Automated Windows, Linux, and macOS archives include `portable.flag`.
- macOS portable settings live beside `MD Editor.app`, preserving app bundle
  contents and ad-hoc signature.
- Platform-config and portable-mode path selection have unit coverage.
- PDF rendering keeps at least 2x supersampling and responds to display scale.
- Search and recovery screens render successfully in headless UI tests.
- Search screenshot: [`../images/search_window.png`](../images/search_window.png)
- Recovery screenshot:
  [`../images/recovery_window.png`](../images/recovery_window.png)
- Recovery error appears once as toast; status bar does not duplicate it.

## Verification Gates

Required before handoff:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
```

## Remaining Platform Validation

Static cross-platform hardening completed 2026-06-07:

- Windows external links use direct `rundll32.exe` invocation; document URLs
  are never passed through `cmd.exe`.
- PDFium build output supports custom Cargo target directories and
  target-triple subdirectories.
- Release automation builds Windows x64, macOS Intel, and macOS Apple Silicon.
- macOS packages use an ad-hoc-signed `.app` bundle with generated `.icns`,
  `Info.plist`, and PDFium under `Contents/Resources`.
- Runtime PDFium discovery covers standard macOS app bundle resources.
- All release archives include application icon and license.

Windows and macOS binaries, DPI behavior, icons, PDFium loading, and workflow
smoke tests were not run on this Linux host. New GitHub Actions package jobs
must pass, then artifacts need host-level smoke tests. Milestone 12 remains open
until those results are recorded.
