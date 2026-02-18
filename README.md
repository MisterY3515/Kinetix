# Kinetix

**Kinetix** is an experimental compiled and interpreted programming language built in Rust, created by [MisterY3515](https://github.com/MisterY3515) as a student project (with AI assistance).

Kinetix source files (`.kix`) can be **interpreted directly** via `kivm exec`, making it quick to prototype and test scripts without a separate compilation step. It can also be **compiled** to register-based bytecode (custom `.exki` or platform-specific `.exe`/`.app`/`.appimage` etc.) and runs on a custom virtual machine (**KiVM**). It supports Windows (Mainly tested), Linux, and macOS on **x86_64**, **ARM64**, and **Apple Silicon** architectures.

> ⚠️ **Development Status:** Kinetix is under active development. Some functions listed in the documentation or standard library may be **incomplete**, **not fully functional**, or **not yet implemented**. APIs and behavior may change between builds.

## What is it for?

Kinetix is designed to make it easy to write **software tools and automation scripts**. Its built-in standard library covers the most common needs out of the box — file I/O, networking, databases, UI, even local AI inference — so you can build working utilities quickly without hunting for external packages.

## Code Examples

### Hello World

```
println("Hello, world!")
```

### Variables

```
let name = "Kinetix"       // immutable
let pi: float = 3.14159    // explicit type
mut counter = 0             // mutable
counter = counter + 1
```

### Functions

```
fn add(a: int, b: int) -> int {
    return a + b
}

fn greet(who: string) -> string {
    return "Hello, " + who + "!"
}

println(add(3, 4))      // 7
println(greet("World")) // Hello, World!
```

### If / Else

```
let score = 85

if score >= 90 {
    println("A")
} else if score >= 80 {
    println("B")
} else {
    println("C")
}
```

### While Loop

```
mut x = 10
while x > 0 {
    println(x)
    x = x - 1
}
```

### For Loop (Arrays)

```
let fruits = ["apple", "banana", "cherry"]

for fruit in fruits {
    println(fruit)
}
```

### For Loop (Range)

```
// range(start, end) returns an array [start..end)
for i in range(0, 10) {
    println(i)
}
```

### Arrays & Builtins

```
let nums = [5, 3, 8, 1, 9, 2]

println(len(nums))      // 6
println(min(nums))      // 1
println(max(nums))      // 9

let sorted = sort(nums)
println(sorted)          // [1, 2, 3, 5, 8, 9]

let reversed = reverse(nums)
println(reversed)
```

### String Operations

```
let text = "Hello, Kinetix!"

println(to_upper(text))              // HELLO, KINETIX!
println(to_lower(text))              // hello, kinetix!
println(contains(text, "Kinetix"))   // true
println(split(text, ", "))           // ["Hello", "Kinetix!"]
println(replace(text, "Hello", "Hi")) // Hi, Kinetix!
println(trim("  spaces  "))         // spaces
```

### Lambda Functions

```
let double = fn(x: int) -> int {
    return x * 2
}

println(double(21))  // 42
```

### Math Module

```
println(Math.sqrt(144.0))        // 12.0
println(Math.abs(-5))            // 5
println(Math.clamp(15, 0, 10))   // 10
println(Math.lerp(0.0, 100.0, 0.5)) // 50.0
println(Math.sin(Math.rad(90.0)))    // 1.0
```

### Multi-File Projects

```
// math_utils.kix
fn square(n: int) -> int {
    return n * n
}
```

```
// main.kix
#include "math_utils.kix"

println(square(7))  // 49
```

### Terminal Colors (Build 5)

```
term.clear()
term.color_print("green", "SUCCESS: All tests passed!")
term.color_print("red", "ERROR: Something went wrong")
println(term.bold("important") + " and " + term.italic("elegant"))
```

### Interactive Shell

```bash
$ kivm shell
Kinetix Shell v0.0.3 build 6

~ ❯ println(2 + 2)
4
~ ❯ ls
~ ❯ cd projects
~/projects ❯ exit
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

## Benchmarks (v0.0.3 Build 6)

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
