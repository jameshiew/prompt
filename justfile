run:
    RUST_LOG=debug cargo run

lint:
    cargo +nightly fmt --all -- --check
    cargo clippy --all-targets
    RUSTDOCFLAGS="-Dwarnings" cargo doc --document-private-items --no-deps

install:
    cargo install --locked --path .

fmt:
    cargo +nightly fmt

help:
    cargo run -- --help