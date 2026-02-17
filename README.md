# Kinetix

A next-generation programming language designed to bridge the gap between Python's readability and C++'s performance.

## Crates

| Crate | Description |
|-------|-------------|
| `language` | Parser, Lexer, AST |
| `kicomp` | Compiler (AST → bytecode) |
| `kivm` | Virtual Machine (register-based) |
| `cli` | Command-line interface |
| `installer` | Cross-platform installer |

## Build

```bash
cargo build --release
```

## Usage

```bash
kivm run app.exki
kivm exec script.kix
```

## License

MIT
