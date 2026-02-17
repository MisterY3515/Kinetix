# Multi-File Example

This example demonstrates how to split a Kinetix project across multiple `.kix` files using `#include`.

## Structure

```
multifile/
├── main.kix          ← Entry point
├── math_utils.kix    ← Math functions (square, cube, factorial, fibonacci...)
└── string_utils.kix  ← String helpers (greet, repeat_str, banner)
```

## Run

```bash
# Interpret directly
kivm exec main.kix

# Compile to bytecode and run
kivm compile -i main.kix -o app.exki
kivm run app.exki

# Create standalone executable
kivm compile -i main.kix --exe
```

The `#include` directive is processed recursively — included files can include other files too.
