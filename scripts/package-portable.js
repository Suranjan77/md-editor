#!/usr/bin/env node
/**
 * package-portable.js
 *
 * Cross-platform script that builds MD Editor in release mode and packages
 * the result into a portable zip archive alongside the documentation files.
 *
 * Usage:
 *   npm run package:portable
 *   node scripts/package-portable.js
 *
 * Output (Linux):   md-editor-v{version}-linux-portable.zip
 * Output (Windows): md-editor-v{version}-portable.zip
 */

import { execSync } from 'child_process';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

// ── Paths ────────────────────────────────────────────────────────────────────

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT      = path.resolve(__dirname, '..');

// ── Config ───────────────────────────────────────────────────────────────────

const pkg     = JSON.parse(fs.readFileSync(path.join(ROOT, 'package.json'), 'utf8'));
const version = pkg.version;
const isWin   = process.platform === 'win32';

const binaryName = isWin ? 'md-editor.exe' : 'md-editor';

// Mirror the naming convention used for the existing Windows zip.
const folderName = isWin
    ? `md-editor-v${version}-portable`
    : `md-editor-v${version}-linux-portable`;

const zipName    = `${folderName}.zip`;
const stageDir   = path.join(ROOT, 'dist-portable');
const stageFolder = path.join(stageDir, folderName);
const zipDest    = path.join(ROOT, zipName);

// Extra files to bundle alongside the binary.
const DOCS = ['USER_GUIDE.md', 'TECHNICAL_DOCS.md'];

// ── Helpers ──────────────────────────────────────────────────────────────────

function run(cmd, opts = {}) {
    execSync(cmd, { cwd: ROOT, stdio: 'inherit', ...opts });
}

function step(emoji, msg) {
    console.log(`\n${emoji}  ${msg}`);
}

// ── Main ─────────────────────────────────────────────────────────────────────

step('📦', `Building md-editor v${version} portable for ${isWin ? 'Windows' : 'Linux'}…`);

// 1. Full release build (frontend via beforeBuildCommand, then Rust).
step('🔨', 'Running cargo tauri build…');
run('cargo tauri build');

// 2. Stage directory.
step('📁', `Staging files into ${stageFolder}`);
fs.rmSync(stageDir, { recursive: true, force: true });
fs.mkdirSync(stageFolder, { recursive: true });

// Copy binary.
const binarySrc = path.join(ROOT, 'src-tauri', 'target', 'release', binaryName);
if (!fs.existsSync(binarySrc)) {
    console.error(`\n❌  Binary not found at: ${binarySrc}`);
    process.exit(1);
}
const binaryDest = path.join(stageFolder, binaryName);
fs.copyFileSync(binarySrc, binaryDest);
if (!isWin) {
    // Ensure the binary is executable on Linux / macOS.
    fs.chmodSync(binaryDest, 0o755);
}

// Copy documentation files (skip gracefully if absent).
for (const doc of DOCS) {
    const src = path.join(ROOT, doc);
    if (fs.existsSync(src)) {
        fs.copyFileSync(src, path.join(stageFolder, doc));
    } else {
        console.warn(`⚠️   ${doc} not found — skipping.`);
    }
}

// 3. Create zip archive.
step('🗜️ ', `Creating ${zipName}…`);

// Remove any previous zip with the same name.
if (fs.existsSync(zipDest)) fs.rmSync(zipDest);

if (isWin) {
    // PowerShell's Compress-Archive is available on Windows 5.0+.
    run(
        `powershell -NoProfile -Command "Compress-Archive -Path '${stageFolder}' -DestinationPath '${zipDest}'"`,
    );
} else {
    // Use the system zip utility; -j would strip paths, so cd into stageDir
    // first so the archive preserves the top-level folder name.
    run(`zip -r "${zipDest}" "${folderName}"`, { cwd: stageDir });
}

// 4. Clean up the staging directory.
fs.rmSync(stageDir, { recursive: true, force: true });

// ── Done ─────────────────────────────────────────────────────────────────────

const stats     = fs.statSync(zipDest);
const sizeMB    = (stats.size / 1024 / 1024).toFixed(2);
step('✅', `Done: ${zipName}  (${sizeMB} MB)`);
