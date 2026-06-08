set shell := ["bash", "-cu"]

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

lint:
    cargo clippy --workspace --all-targets -- -D warnings

test:
    cargo test --workspace

architecture:
    ./scripts/architecture-check.sh

metrics:
    ./scripts/architecture-metrics.sh

benchmark:
    ./scripts/refactor-benchmark.sh

check: architecture fmt-check lint test
