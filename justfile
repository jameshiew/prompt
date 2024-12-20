run:
    RUST_LOG=debug cargo run

install:
    cargo install --locked --path .

fmt:
    cargo +nightly fmt

help:
    cargo run -- --help