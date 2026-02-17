# Kinetix

Kinetix is a sperimental compiled and interpretated language made in Rust for experimental purposes with help of AI made by me, [MisterY3515](https://github.com/MisterY3515) as student.

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

See [LICENSE](LICENSE).