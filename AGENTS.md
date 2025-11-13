## Coding guidelines

- Keep `main.rs` files minimal
- Format using `cargo +nightly fmt`
- Use `format!("{var}")` over `format!("{}", var)`
- Only use `#[allow(dead_code)]` when truly needed
- Favour `just` commands over `cargo`
- Guard against numeric over/underflow (use saturating ops)

## Dependencies

- Use `cargo add` when adding new dependencies, to ensure we're using the latest compatible version
- Prefer using features that will be easier to build (e.g. rustls over openssl)
- Run `just dep-check` when changing dependencies and fix any issues

## When finishing a task

- Run `just test`
- Run `just clippy` - fix issues
- Finally, run `just fmt`
- Update docs as needed
- Add to the "Learnings" section of AGENTS.md as appropriate - revise/update existing learnings if necessary
- Propose next steps

## Learnings

- Avoid `serde_yml`/`libyml` because they're flagged by RUSTSEC-2025-0067/0068; prefer maintained YAML serializers (e.g. `serde_norway`).
