run:
    cargo run

install:
    RUSTFLAGS="-C target-cpu=native" cargo install --profile installation --path .

fmt:
    cargo +nightly fmt