#![cfg(test)]
use kinetix_language::parser::Parser;
use kinetix_language::lexer::Lexer;
use bumpalo::Bump;
use crate::compiler::Compiler;
use std::time::Instant;

#[test]
fn benchmark_10k_struct_instantiations() {
    // Generate AST source code dynamically with 10k struct instantiations
    let mut source = String::new();
    source.push_str("struct Packet { id: int, data: int }\n");
    source.push_str("fn main() {\n");
    for i in 0..10000 {
        source.push_str(&format!("    let p{} = Packet {{ id: {}, data: {} }};\n", i, i, i));
    }
    source.push_str("}\n");

    let start = Instant::now();
    
    // 1. Parsing Phase
    let lexer = Lexer::new(&source);
    let arena = Bump::new();
    let mut parser = Parser::new(lexer, &arena);
    let ast = parser.parse_program();
    if !parser.errors.is_empty() {
        panic!("Parsing failed: {:?}", parser.errors);
    }

    // 2. Compilation Phase (Symbol Resolution, Typeck, MIR, Borrowck, Drop, SSA)
    let mut compiler = Compiler::new();
    match compiler.compile(&ast.statements, None) {
        Ok(_) => {},
        Err(e) => panic!("Compilation failed: {}", e),
    }

    let duration = start.elapsed();
    println!("Successfully compiled 10,000 structural instantiations in {:?}", duration);
    
    // Assert Big-O scaling constraint (O(n) implies it should be well under 2 seconds on any modern CI)
    assert!(duration.as_millis() < 2500, "Compilation is scaling poorly! Took {:?}! Limit is 2.5s", duration);
}
