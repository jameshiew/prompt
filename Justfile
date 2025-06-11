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

install-cargo-tools:
    cargo install cargo-binstall
    cargo binstall --no-confirm just
    cargo binstall --no-confirm cargo-hack
    cargo binstall --no-confirm cargo-nextest
    cargo binstall --no-confirm cargo-machete
    cargo binstall --no-confirm cargo-audit

test:
    cargo nextest run --future-incompat-report

## 3p

machete:
    cargo machete

audit:
    cargo audit