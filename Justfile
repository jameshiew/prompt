## dev
    
debug:
    RUST_LOG=debug cargo run

release:
    cargo run --release

install:
    cargo install --locked --path .

help:
    cargo run -- --help

fmt:
    cargo +nightly fmt

build:
    cargo build --all-targets

bench:
    cargo bench --all-targets

fmt-check:
    cargo +nightly fmt --all -- --check

check:
    cargo check --all-targets

clippy:
    cargo clippy --all-targets -- -D warnings

doc:
    RUSTDOCFLAGS="-Dwarnings" cargo doc --document-private-items --no-deps

lint: fmt-check clippy

test:
    cargo nextest run

## 3p

machete:
    cargo machete

audit:
    cargo audit