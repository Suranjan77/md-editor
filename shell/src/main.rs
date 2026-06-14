//! md-editor shell. Startup builds the default registry and keymap; a binding
//! conflict makes the process exit non-zero (plan §3.1: conflicts detected
//! at startup). Modes:
//!
//! - default / `<vault-dir>`: the iced GUI (ADR-0100) over the given vault
//!   (current directory if omitted).
//! - `--dump-shortcuts` prints the shortcuts table *generated from the
//!   command registry* — the single source of truth; docs/SHORTCUTS.md is
//!   produced by this, never edited by hand.
//! - `--palette <query>` exercises the registry-backed palette.
//! - `--demo` walks the BUG-A/BUG-C scenario end to end on the real kernel,
//!   headless (used by CI).

use std::process::ExitCode;

use md_kernel::defaults::default_registry;
use md_shell::{gui, headless};

fn main() -> ExitCode {
    let registry = match default_registry() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("md-editor: command registry is invalid: {e}");
            return ExitCode::FAILURE;
        }
    };
    let keymap = match registry.keymap() {
        Ok(k) => k,
        Err(e) => {
            eprintln!("md-editor: keymap conflict: {e}");
            return ExitCode::FAILURE;
        }
    };

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--dump-shortcuts") => headless::dump_shortcuts(&registry),
        Some("--palette") => {
            headless::palette(&registry, args.get(1).map(String::as_str).unwrap_or(""))
        }
        Some("--demo") => return headless::demo(&keymap),
        Some("--install-desktop") => {
            if !cfg!(target_os = "linux") {
                println!("not supported");
                return ExitCode::SUCCESS;
            }
            match md_shell::desktop::install() {
                Ok(()) => {
                    println!("Desktop entry and icons installed successfully.");
                }
                Err(e) => {
                    eprintln!("md-editor: desktop installation failed: {e}");
                    return ExitCode::FAILURE;
                }
            }
        }
        Some("--uninstall-desktop") => {
            if !cfg!(target_os = "linux") {
                println!("not supported");
                return ExitCode::SUCCESS;
            }
            match md_shell::desktop::uninstall() {
                Ok(()) => {
                    println!("Desktop entry and icons uninstalled successfully.");
                }
                Err(e) => {
                    eprintln!("md-editor: desktop uninstallation failed: {e}");
                    return ExitCode::FAILURE;
                }
            }
        }
        Some("--help") | Some("-h") => {
            println!(
                "usage: md-editor [<vault-dir> | --dump-shortcuts | --palette <query> | --demo | --install-desktop | --uninstall-desktop]"
            );
        }
        first => {
            let requested = first.map(std::path::PathBuf::from);
            let root = requested
                .as_ref()
                .and_then(|path| path.canonicalize().ok())
                .filter(|path| path.is_dir());
            let Some(root) = root else {
                let message = requested
                    .map(|path| format!("Vault folder is unavailable: {}", path.display()));
                if let Err(e) = gui::welcome::run_startup(registry, keymap, message) {
                    eprintln!("md-editor: {e}");
                    return ExitCode::FAILURE;
                }
                return ExitCode::SUCCESS;
            };
            md_shell::vault_picker::record_recent(&root);
            // User remaps (plan §3.1): bad rows warn, never block startup.
            let mut keymap = keymap;
            let report = md_shell::settings::apply_keymap_overrides(&root, &registry, &mut keymap);
            for warning in &report.warnings {
                eprintln!("md-editor: {warning}");
            }
            if let Err(e) = gui::run(registry, keymap, root) {
                eprintln!("md-editor: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    ExitCode::SUCCESS
}
