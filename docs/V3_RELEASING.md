# V3 Release Process

V3 packages are built from the independent `v3/` workspace.

## Artifacts

| OS | Portable | Installed |
| --- | --- | --- |
| Windows x64 | ZIP | NSIS per-user installer |
| Linux x64 | tar.gz | AppImage |
| macOS Intel | tar.gz containing `.app` | DMG |
| macOS Apple Silicon | tar.gz containing `.app` | DMG |

Portable archives contain `portable.flag`. AppImage is inherently portable.
Global state (`recent-vaults.json`, `tracker.db`) stays beside package or
AppImage. Vault state remains in `<vault>/.md3`, so moving a vault preserves
index, annotations, settings, and session state.

Installers omit `portable.flag`; global state uses platform config directory.
PDFium and its license files ship in every artifact.

## Local Build

Set `PDFIUM_LIB` to platform PDFium shared library. Set
`PDFIUM_LICENSE_DIR` to extracted PDFium `licenses/` directory.

```bash
cd v3
cargo xtask dist
```

Use `cargo xtask dist --archive-only` when installer tooling is unavailable.
Output lands in `v3/dist/` with `SHA256SUMS`.

Required host tools:

- Windows: PowerShell, NSIS.
- Linux: `tar`, `linuxdeploy`, `appimagetool`.
- macOS: `tar`, `sips`, `iconutil`, `codesign`, `hdiutil`.

## Release

1. Replace `3.0.0-dev` in `v3/Cargo.toml` with release version.
2. Run root and v3 quality gates.
3. Commit version and release notes.
4. Tag exact version as `v3.X.Y`.
5. Push tag. `V3 release packages` creates draft GitHub release.
6. Download each artifact. Verify `sha256sum -c SHA256SUMS`.
7. Smoke portable and installed artifact on each target.
8. Confirm PDF render, recent-vault persistence, tracker persistence, restart,
   vault move, and uninstall behavior.
9. Publish draft after platform signoff.

## Signing

Current macOS app uses ad-hoc signing. Windows installer is unsigned.
Public production release still requires organization certificates:

- Windows Authenticode-sign executable and installer.
- macOS Developer ID sign, notarize, and staple DMG.

Never publish unsigned artifacts as trusted/signed builds.
