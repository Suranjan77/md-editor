# Release Process

Release packages are produced by `cargo xtask dist`, which builds the
`md-editor` binary with the `pdfium` feature, stages the PDFium library and
license files, and creates the platform archives and installers.

## Artifacts

| OS | Portable | Installed |
| --- | --- | --- |
| Windows x64 | ZIP | NSIS per-user installer |
| Linux x64 | tar.gz | AppImage |
| macOS Intel | tar.gz containing `.app` | DMG |
| macOS Apple Silicon | tar.gz containing `.app` | DMG |

Portable archives contain `portable.flag`; AppImage is inherently portable.
Global state (`recent-vaults.json`, `tracker.db`) stays beside the package or
AppImage. Per-vault state remains in `<vault>/.md-editor/`, so moving a vault
preserves index, annotations, settings, and session state.

Installers omit `portable.flag`; global state then uses the platform config
directory. PDFium and its license files ship in every artifact.

## Local build

Set `PDFIUM_LIB` to the platform PDFium shared library, and `PDFIUM_LICENSE_DIR`
to the extracted PDFium `licenses/` directory, then:

```bash
cargo xtask dist
```

Use `cargo xtask dist --archive-only` when installer tooling is unavailable.
Output lands in `dist/` with a `SHA256SUMS` file.

Required host tools:

- Windows: PowerShell, NSIS.
- Linux: `tar`, `linuxdeploy`, `appimagetool`.
- macOS: `tar`, `sips`, `iconutil`, `codesign`, `hdiutil`.

## Cutting a release

1. Replace `3.0.0-dev` in `Cargo.toml` (`[workspace.package] version`) with the
   release version.
2. Run the quality gate (`just check`, or the commands in
   [TESTING.md](TESTING.md#full-pre-handoff-gate)).
3. Commit the version bump and release notes.
4. Tag the exact version, e.g. `v3.0.0`.
5. Push the tag. The **Release packages** workflow
   (`.github/workflows/release.yml`) builds every target and drafts a GitHub
   release.
6. Download each artifact and verify `sha256sum -c SHA256SUMS`.
7. Smoke-test the portable and installed artifact on each target: confirm PDF
   rendering, recent-vault persistence, tracker persistence, restart restore,
   vault move, and uninstall behavior.
8. Publish the draft after platform signoff.

## Signing

The macOS app currently uses ad-hoc signing and the Windows installer is
unsigned. A public production release still requires organization certificates:

- Windows: Authenticode-sign the executable and installer.
- macOS: Developer ID sign, notarize, and staple the DMG.

Never publish unsigned artifacts as trusted/signed builds.
