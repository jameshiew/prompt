run *args:
    cargo run {{args}}

clippy:
    cargo clippy --all-targets -- -D warnings

lint: clippy
    actionlint
    pinact run --fix=false --no-api

pin-actions:
    pinact run

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

install:
    cargo auditable install --locked --path .
