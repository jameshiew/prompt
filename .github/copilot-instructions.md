## Overview

`prompt` is a Rust CLI tool that concatenates and formats files from a directory into a single prompt suitable for Large Language Models (LLMs). It respects `.gitignore` and `.promptignore` patterns, counts tokens using OpenAI's tiktoken, and supports multiple output formats (plaintext, JSON, YAML).

## Development Commands

Use `just` to run any development commands rather than tools like `cargo`. Most essential commands:

```
just run
just check
just test
```

Check `Justfile` for more.

### Important Dependencies

- `tiktoken-rs`: OpenAI tokenizer (o200k_base model) for accurate token counting
- `ignore`: Provides gitignore-style pattern matching and efficient directory walking
- `arboard`: Cross-platform clipboard support
- `bindet`: Binary file detection to exclude non-text files

### Testing Strategy

When adding new features:

1. Unit tests go in the same file as the implementation (see existing `#[cfg(test)]` modules)
2. Integration tests for CLI commands should test both stdout output and clipboard behavior

## Doing a task

First, briefly describe in a sentence what MCP tools are available to you (if any).

When finishing a task:

- run `just clippy` and fix any errors/warnings
- run `just fmt` to format the code
- update `README.md` if relevant
