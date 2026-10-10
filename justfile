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

# Run the explicitly authorized, ignored Oracle Free live proof.
live:
    cargo test --locked -p kuberic-oracle-tests --test live_observer -- --ignored --exact observes_owned_oracle_free_member --nocapture

# Inspect resources owned by the Oracle Free live proof.
live-inspect:
    docker -H "${KUBERIC_ORACLE_DOCKER_HOST:-unix:///var/run/docker.sock}" ps -a --filter name=kuberic-oracle-poc
    docker -H "${KUBERIC_ORACLE_DOCKER_HOST:-unix:///var/run/docker.sock}" network ls --filter name=kuberic-oracle-poc
    docker -H "${KUBERIC_ORACLE_DOCKER_HOST:-unix:///var/run/docker.sock}" volume ls --filter name=kuberic-oracle-poc

# Recover and clean an interrupted fixture from its identity journal.
live-cleanup:
    cargo run --locked -p kuberic-oracle-tests --bin oracle-poc-cleanup
