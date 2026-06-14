set shell := ["bash", "-cu"]

# Run the app over a vault folder (current directory by default).
run vault=".":
    cargo run -- {{vault}}

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

lint:
    cargo clippy --workspace --all-targets -- -D warnings

test:
    cargo test --workspace

# Verify engine crates stay toolkit-free (ADR-0100).
architecture:
    ./scripts/architecture-check.sh

# Enforce per-module line-count ratchets (budgets.toml).
budget:
    ./scripts/size-budget.sh

# Regenerate the keyboard-shortcuts reference from the command registry.
shortcuts:
    cargo run -q -p md-shell -- --dump-shortcuts > docs/SHORTCUTS.md

check: architecture budget fmt-check lint test
