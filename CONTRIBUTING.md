# Contributing to cliproot-rust

Thank you for your interest in contributing to ClipRoot!

## Project Structure

ClipRoot is split across two repositories under the [cliproot](https://github.com/cliproot) GitHub organization:

| Repository | Purpose |
|---|---|
| [cliproot/cliproot](https://github.com/cliproot/cliproot) | Protocol specification, canonical JSON Schemas, TypeScript packages (browser SDK, Tiptap extension, browser extension), and web playground |
| [cliproot/cliproot_rust](https://github.com/cliproot/cliproot_rust) (this repo) | Native runtime — CLI, MCP server, storage engine, OS clipboard integration |

Both repositories implement **CRP v0.0.3** independently. The canonical protocol definition is the [JSON Schema](https://github.com/cliproot/cliproot/tree/main/schema) in the `cliproot` repo. The human-readable [specification](https://github.com/cliproot/cliproot/tree/main/spec) lives there as well.

### When to contribute where

- **Protocol changes** (new entity types, schema fields, hashing algorithm changes) — start in [cliproot/cliproot](https://github.com/cliproot/cliproot) with a schema and spec update. Both implementations will then need to be updated.
- **Rust implementation changes** (CLI commands, MCP tools, storage, performance) — contribute here.
- **TypeScript/browser changes** (browser SDK, extension, Tiptap, playground) — contribute to [cliproot/cliproot](https://github.com/cliproot/cliproot).

## Development Setup

### Prerequisites

- Rust 1.88+ (pinned via `rust-toolchain.toml`)
- No system SQLite needed — it's bundled via `libsqlite3-sys`

### Build and Test

```bash
cargo check --workspace        # verify compilation
cargo build -p cliproot-cli    # build the CLI binary
cargo test --workspace         # run all tests
```

### Code Quality

These checks run in CI on every push and PR:

```bash
cargo fmt --all                # auto-fix formatting
cargo fmt --all -- --check     # check formatting (CI)
cargo clippy --workspace -- -D warnings   # lint
```

## Pull Request Guidelines

1. **Run CI checks locally** before opening a PR: `cargo fmt --all -- --check && cargo clippy --workspace -- -D warnings && cargo test --workspace`
2. **Keep PRs focused.** One logical change per PR. If a PR touches multiple crates, explain why in the description.
3. **Add tests** for new functionality. The `cliproot-store` crate has integration tests in `crates/cliproot-store/tests/`.
4. **Update the README** if you add or change CLI commands or MCP tools.
5. **Follow existing patterns.** Look at how similar features are implemented before introducing new abstractions.

## Workspace Layout

```
crates/
  cliproot-core/       # protocol model, hashing, verification, text matching
  cliproot-store/      # filesystem object store + SQLite index, pack format
  cliproot-mcp/        # MCP server library (24+ tools) + standalone binary
  cliproot-clipboard/  # OS clipboard integration
  cliproot-cli/        # clap-based CLI binary, includes embedded MCP server
skills/
  cliproot-research/   # Agent Skills package (embedded into binary at build time)
```

**Dependency graph:** `cli -> mcp -> store -> core`

## Reporting Issues

Open an issue on this repository for bugs or feature requests related to the Rust implementation. For protocol-level issues (schema, spec, hashing), open an issue on [cliproot/cliproot](https://github.com/cliproot/cliproot).

## License

By contributing, you agree that your contributions will be licensed under the [Apache License 2.0](LICENSE).
