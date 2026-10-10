export CARGO_BUILD_JOBS := env_var_or_default("CARGO_BUILD_JOBS", "1")
export PATH := env_var("HOME") + "/.local/bin:" + env_var("HOME") + "/.cargo/bin:" + env_var("PATH")

# Run all server-free validation.
default: check

# Check formatting, strict Clippy, and tests.
check: fmt-check clippy test

# Check Rust formatting without changing files.
fmt-check:
    cargo fmt --all -- --check

# Run strict Clippy for every target and feature.
clippy:
    cargo clippy --locked --workspace --all-targets --all-features -- -D warnings

# Run all server-free tests.
test:
    cargo test --locked --workspace --all-features
