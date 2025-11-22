# prompt

Experimental tool for concatenating and formatting files under the current directory into an LLM prompt, respecting `.gitignore` and `.promptignore` files by default.

## Installation

```shell
cargo install --locked --git https://github.com/jameshiew/prompt
```

## Why?

When asking a chat-based LLM like ChatGPT o1 something about code, you want to provide as much context as possible - often the limiting factor is the size of the context window. I'm using this tool as something I can easily add functionality to as needed, e.g. being able to count tokens for individual files in order to be able to work out which files to trim from a prompt, or being able to easily ignore certain file types. For a more mature tool, see <https://github.com/mufeedvh/code2prompt>.

## Basic usage

```shell
prompt # copies straight to clipboard and prints summary
prompt --format json --stdout # prints prompt content as json to stdout
prompt -p src/ app/ -e out/  # include/exclude certain paths/globs
prompt --no-gitignore        # include files that are normally skipped by gitignore
```
