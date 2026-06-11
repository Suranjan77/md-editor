//! md3 shell. Startup builds the default registry and keymap; a binding
//! conflict makes the process exit non-zero (plan §3.1: conflicts detected
//! at startup). Modes:
//!
//! - default / `<vault-dir>`: the iced GUI (ADR-0100) over the given vault
//!   (current directory if omitted).
//! - `--dump-shortcuts` prints the shortcuts table *generated from the
//!   command registry* — the single source of truth; docs/V3_SHORTCUTS.md is
//!   produced by this, never edited by hand.
//! - `--palette <query>` exercises the registry-backed palette.
//! - `--demo` walks the BUG-A/BUG-C scenario end to end on the real kernel,
//!   headless (used by CI).

use std::process::ExitCode;

use md3_kernel::defaults::default_registry;
use md3_shell::{gui, headless};

fn main() -> ExitCode {
    let registry = match default_registry() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("md3: command registry is invalid: {e}");
            return ExitCode::FAILURE;
        }
    };
    let keymap = match registry.keymap() {
        Ok(k) => k,
        Err(e) => {
            eprintln!("md3: keymap conflict: {e}");
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
        Some("--help") | Some("-h") => {
            println!(
                "usage: md3-shell [<vault-dir> | --dump-shortcuts | --palette <query> | --demo]"
            );
        }
        first => {
            let root = std::path::PathBuf::from(first.unwrap_or("."));
            let root = match root.canonicalize() {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("md3: vault {}: {e}", root.display());
                    return ExitCode::FAILURE;
                }
            };
            if let Err(e) = gui::run(registry, keymap, root) {
                eprintln!("md3: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    ExitCode::SUCCESS
}
