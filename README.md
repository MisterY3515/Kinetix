# Kinetix

**Kinetix** is an experimental compiled and interpreted programming language built in Rust, created by [MisterY3515](https://github.com/MisterY3515) as a student project (with AI assistance).

Kinetix source files (`.kix`) can be **interpreted directly** via `kivm exec`, making it quick to prototype and test scripts without a separate compilation step. It can also be **compiled** to register-based bytecode (custom `.exki` or platform-specific `.exe`/`.app`/`.appimage` etc.) and runs on a custom virtual machine (**KiVM**). It supports Windows (Mainly tested), Linux, and macOS on **x86_64**, **ARM64**, and **Apple Silicon** architectures.

## What is it for?

Kinetix is designed to make it easy to write **software tools and automation scripts**. Its built-in standard library covers the most common needs out of the box — file I/O, networking, databases, UI, even local AI inference — so you can build working utilities quickly without hunting for external packages.

### Built-in Libraries

| Module | What it does |
|--------|-------------|
| **Math** | Trigonometry, vectors, matrices, random numbers, clamp/lerp |
| **System** | CPU/memory info, shell commands, clipboard, hostname, OS detection |
| **Data** | Read/write files (text & bytes), JSON parse/stringify, CSV parse/write |
| **Net** | HTTP GET/POST requests, file downloads |
| **Graph** | Open native windows, draw pixels, immediate-mode UI (buttons, labels, text input), line plots |
| **Audio** | One-shot and streaming audio playback |
| **Crypto** | SHA-256 hashing, HMAC, UUID generation, random bytes |
| **DB** | SQLite database (connect, query, execute) |
| **LLM** | Local AI inference via Ollama (chat, generate) |

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