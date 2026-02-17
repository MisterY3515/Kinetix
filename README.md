# Kinetix

**Kinetix** is an experimental compiled and interpreted programming language built in Rust, created by [MisterY3515](https://github.com/MisterY3515) as a student project (with AI assistance).

Kinetix source files (`.kix`) can be **interpreted directly** via `kivm exec`, making it quick to prototype and test scripts without a separate compilation step. It can also be **compiled** to register-based bytecode (custom `.exki` or platform-specific `.exe`/`.app`/`.appimage` etc.) and runs on a custom virtual machine (**KiVM**). It supports Windows (Mainly tested), Linux, and macOS on **x86_64**, **ARM64**, and **Apple Silicon** architectures.

> ⚠️ **Development Status:** Kinetix is under active development. Some functions listed in the documentation or standard library may be **incomplete**, **not fully functional**, or **not yet implemented**. APIs and behavior may change between builds.

## What is it for?

Kinetix is designed to make it easy to write **software tools and automation scripts**. Its built-in standard library covers the most common needs out of the box — file I/O, networking, databases, UI, even local AI inference — so you can build working utilities quickly without hunting for external packages.

## Code Examples

### Hello World

```kix
println("Hello, world!")
```

### Variables and Functions

```kix
let name = "Kinetix"
mut counter = 0

fn greet(who: string) -> string {
    return "Hello, " + who + "!"
}

println(greet(name))
```

### Loops and Control Flow

```kix
// For loop with range
for i in 0..10 {
    println(i)
}

// While loop
mut x = 100
while x > 1 {
    x = x / 2
    println(x)
}

// Conditionals
let score = 85
if score >= 90 {
    println("A")
} else if score >= 80 {
    println("B")
} else {
    println("C")
}
```

### Arrays and Iteration

```kix
let fruits = ["apple", "banana", "cherry"]

for fruit in fruits {
    println(fruit)
}

let lengths = map(fruits, fn(f) -> int { return len(f) })
println(lengths)  // [5, 6, 6]
```

### Classes

```kix
class Vector2 {
    pub x: float
    pub y: float

    fn length() -> float {
        return Math.sqrt(x * x + y * y)
    }

    fn add(other: Vector2) -> Vector2 {
        return Vector2(x + other.x, y + other.y)
    }
}

let a = Vector2(3.0, 4.0)
println(a.length())  // 5.0
```

### File I/O and JSON

```kix
// Write and read JSON
let config = { "theme": "dark", "fontSize": 14 }
data.write_text("config.json", json.stringify(config))

let loaded = json.parse(data.read_text("config.json"))
println(loaded["theme"])  // dark
```

### HTTP Requests

```kix
let response = net.get("https://api.example.com/data")
let data = json.parse(response)
println(data)
```

### Terminal Colors (new in Build 5)

```kix
term.clear()
term.color_print("green", "SUCCESS: All tests passed!")
term.color_print("red", "ERROR: Something went wrong")

let styled = term.bold("important") + " and " + term.italic("elegant")
println(styled)
```

### Interactive Shell

```bash
$ kivm shell
Kinetix Shell v0.0.2 build 5
Type exit to quit, help for commands.

~ ❯ ls
~ ❯ cd projects
~/projects ❯ println(2 + 2)
4
~/projects ❯ let x = Math.sqrt(144)
~/projects ❯ println(x)
12
```

## Built-in Libraries

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
| **Term** | ANSI colors, cursor control, bash-like commands (ls, cd, cat, grep...) |

## Tooling

| Tool | Command | Description |
|------|---------|-------------|
| **Interpreter** | `kivm exec script.kix` | Run a `.kix` source file directly |
| **Bytecode Runner** | `kivm run app.exki` | Run compiled bytecode |
| **Compiler** | `kivm compile -i src.kix -o out.exki` | Compile to `.exki` bytecode |
| **Bundler** | `kivm compile -i src.kix --exe` | Create a standalone executable |
| **Shell** | `kivm shell` | Interactive terminal with bash-like commands + Kinetix eval |
| **Docs** | `kivm docs` | Open offline documentation in the browser |
| **Tests** | `kivm test ./tests` | Run unit tests in a directory |
| **Version** | `kivm version` | Show version and build info |

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

## Benchmarks (v0.0.2 Build 5)

Parser speed tested on a synthetic source of **3,650 lines** (~70 KB) containing variables, functions, classes, loops, expressions, and arrays.

| Metric | Result |
|--------|--------|
| Average parse time | **6.33 ms** |
| Throughput | **~576,000 lines/sec** |
| Throughput | **~10.8 MB/sec** |
| Statements parsed | 2,350 |

Run the benchmark yourself:

```bash
cargo test -p kinetix-language bench_parser_speed -- --nocapture
```

## Contributing & Issues

Found a bug? Have an idea for a new feature? Please open an issue on the [GitHub Issues](https://github.com/MisterY3515/Kinetix/issues) page.

- 🐛 **Bug reports** — describe what happened and how to reproduce it
- 💡 **Feature requests** — explain the use case and your proposed solution
- 📝 **Questions** — ask anything about the language, tooling, or internals

## License

See [LICENSE](LICENSE).
