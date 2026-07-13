/// Build 38 Phase B2 differential test harness.
///
/// Compiles the same source through both the existing AST-walking
/// `compiler.rs` path (the pipeline's only default today) and the new
/// MIR-consuming `mir_codegen.rs` path (HIR -> MIR -> the 4 MIR validators ->
/// `mir_codegen::compile_mir_program`, none of it wired into the default
/// pipeline anywhere), runs both through the real VM, and asserts they
/// produce identical `print`/`println` output.
///
/// Lives here (in `kivm`'s integration tests) rather than in `kicomp`
/// because `kicomp` cannot depend on `kivm` (it's the other way around) --
/// this is the first point in the dependency graph where both the codegen
/// paths and a real VM to run them on are available together.
///
/// Scoped deliberately to MIR-representable constructs only: arithmetic,
/// `let`/assign, `if`/`while`/`for`/`break`/`continue`, simple function
/// calls. Struct literals, `match`, methods, and closures are known,
/// documented gaps in MIR itself (see `mir_codegen.rs`'s module doc comment)
/// and are out of scope here, not silently miscompared.
use bumpalo::Bump;
use kinetix_language::lexer::Lexer;
use kinetix_language::parser::Parser;
use kinetix_kicomp::compiler::Compiler;
use kinetix_kicomp::hir::lower_to_hir;
use kinetix_kicomp::ir::CompiledProgram;
use kinetix_kicomp::mir::lower_to_mir;
use kinetix_kicomp::symbol::resolve_program;
use kinetix_kicomp::trait_solver::TraitEnvironment;
use kinetix_kicomp::typeck::TypeContext;
use kinetix_kivm::vm::VM;

fn compile_via_ast(src: &str) -> CompiledProgram {
    let arena = Bump::new();
    let lexer = Lexer::new(src);
    let mut parser = Parser::new(lexer, &arena);
    let program = parser.parse_program();
    let symbols = resolve_program(&program.statements).expect("symbol resolution failed");
    let traits = TraitEnvironment::new();
    let hir = lower_to_hir(&program.statements, &symbols, &traits);
    let mut ctx = TypeContext::new();
    let constraints = ctx.collect_constraints(&hir);
    ctx.solve(&constraints).expect("type checking failed");

    let mut compiler = Compiler::new();
    compiler.compile(&program.statements, None).expect("AST codegen failed");
    compiler.program.clone()
}

fn compile_via_mir(src: &str) -> CompiledProgram {
    let arena = Bump::new();
    let lexer = Lexer::new(src);
    let mut parser = Parser::new(lexer, &arena);
    let program = parser.parse_program();
    let symbols = resolve_program(&program.statements).expect("symbol resolution failed");
    let traits = TraitEnvironment::new();
    let hir = lower_to_hir(&program.statements, &symbols, &traits);
    let mut ctx = TypeContext::new();
    let constraints = ctx.collect_constraints(&hir);
    ctx.solve(&constraints).expect("type checking failed");

    let mir = lower_to_mir(&hir, &ctx.substitution);
    kinetix_kicomp::borrowck::check_mir(&mir).expect("borrow checker rejected valid MIR");
    let mir = kinetix_kicomp::monomorphize::monomorphize(&mir).expect("monomorphization failed");
    kinetix_kicomp::mono_validate::validate(&mir).expect("post-mono validation failed");
    kinetix_kicomp::drop_verify::verify(&mir).expect("drop verification failed");
    kinetix_kicomp::ssa_validate::validate(&mir).expect("SSA validation failed");

    kinetix_kicomp::mir_codegen::compile_mir_program(&mir).expect("MIR codegen failed")
}

fn run_and_capture_output(program: CompiledProgram) -> Vec<String> {
    let mut vm = VM::new(program);
    vm.run().expect("VM execution failed");
    vm.output
}

fn assert_same_output(src: &str) {
    let out_ast = run_and_capture_output(compile_via_ast(src));
    let out_mir = run_and_capture_output(compile_via_mir(src));
    assert_eq!(
        out_ast, out_mir,
        "AST-walking compiler.rs and MIR codegen disagree for:\n{}",
        src
    );
}

#[test]
fn arithmetic_and_let() {
    assert_same_output("let a = 1 + 2 * 3\nprintln(a)");
}

#[test]
fn string_and_boolean_literals() {
    assert_same_output("let s = \"hello\"\nlet b = true\nprintln(s)\nprintln(b)");
}

#[test]
fn if_else_expression() {
    assert_same_output("let x = if 2 > 1 { \"yes\" } else { \"no\" }\nprintln(x)");
}

#[test]
fn if_else_expression_false_branch() {
    assert_same_output("let x = if 1 > 2 { \"yes\" } else { \"no\" }\nprintln(x)");
}

#[test]
fn while_loop() {
    assert_same_output(
        "let mut i = 0\nlet mut sum = 0\nwhile i < 5 {\n    sum = sum + i\n    i = i + 1\n}\nprintln(sum)"
    );
}

#[test]
fn for_loop_over_array() {
    assert_same_output(
        "let arr = [10, 20, 30]\nlet mut total = 0\nfor x in arr {\n    total = total + x\n}\nprintln(total)"
    );
}

#[test]
fn for_loop_over_range() {
    assert_same_output(
        "let mut total = 0\nfor i in 0..5 {\n    total = total + i\n}\nprintln(total)"
    );
}

#[test]
fn function_call() {
    assert_same_output("fn add(a: int, b: int) -> int {\n    return a + b\n}\nprintln(add(4, 5))");
}

#[test]
fn recursive_function_call() {
    assert_same_output(
        "fn fact(n: int) -> int {\n    if n <= 1 {\n        return 1\n    }\n    return n * fact(n - 1)\n}\nprintln(fact(5))"
    );
}

#[test]
fn nested_break_continue() {
    assert_same_output(
        "let mut total = 0\nfor i in 0..10 {\n    if i > 6 {\n        break\n    }\n    if i % 2 == 0 {\n        continue\n    }\n    total = total + i\n}\nprintln(total)"
    );
}

#[test]
fn array_index_and_len() {
    assert_same_output("let arr = [1, 2, 3, 4]\nprintln(arr[2])\nprintln(len(arr))");
}
