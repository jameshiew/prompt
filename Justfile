run *args:
    cargo run {{args}}

clippy:
    cargo clippy --all-targets -- -D warnings

test:
    cargo nextest run

fmt:
    cargo +nightly fmt --all
    tombi format

fmt-check:
    cargo +nightly fmt --all -- --check
    tombi lint

dep-check:
    cargo machete
    cargo deny check
    cargo audit