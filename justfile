run:
    RUST_LOG=debug cargo run

install:
    RUSTFLAGS="-C target-cpu=native" cargo install --profile installation --path .

fmt:
    cargo +nightly fmt

help:
    cargo run -- --help