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

- Keep `deny.toml` limited to policy that differs from cargo-deny defaults.
- Avoid `serde_yml`/`libyml` because they're flagged by RUSTSEC-2025-0067/0068; prefer maintained YAML serializers (e.g. `serde_norway`).
- For overlapping include roots, evaluate explicit exclusions relative to every matching root. Merge by file path and let `excluded = true` win.
- For overlapping include roots, apply `.promptignore` files from the shallowest root so parent rules remain active and nested whitelist rules have deterministic precedence.
- clap flags with an optional value (`num_args = 0..=1`) also need `default_missing_value`, otherwise the bare flag errors with "required argument was not provided".
- Use `subcommand_precedence_over_arg` when a variadic global option can precede a subcommand.
- Convert trailing-slash CLI exclusions to descendant globs because discovery matches files, not directories.
- Pass each `--exclude` pattern as a separate argument. A comma remains part of a literal pattern.
- Define subcommand-specific flags on the applicable `Command` variant so clap rejects them for other commands.
