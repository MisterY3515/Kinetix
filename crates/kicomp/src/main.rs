/// KiComp CLI - Kinetix Compiler
/// Compiles .kix source files into .exki bytecode bundles for KiVM.

use clap::Parser as ClapParser;
use kinetix_language::lexer::Lexer;
use kinetix_language::parser::Parser;
use kinetix_kicomp::compiler::Compiler;
use kinetix_kicomp::exn;
use std::fs;
use std::path::PathBuf;
use bumpalo::Bump;

#[derive(ClapParser)]
#[command(name = "kicomp")]
#[command(about = "Kinetix Compiler — compile .kix to .exki bytecode")]
struct Cli {
    /// Input .kix source file
    #[arg(short, long)]
    input: PathBuf,

    /// Output .exki file (default: same name as input with .exki extension)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Show version information
    #[arg(long)]
    version: bool,
}

fn main() {
    let cli = Cli::parse();

    if cli.version {
        let build = option_env!("KINETIX_BUILD").unwrap_or("Dev");
        println!("Kinetix Compiler v{} ({})", env!("CARGO_PKG_VERSION"), build);
        return;
    }

    let source = match fs::read_to_string(&cli.input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {}: {}", cli.input.display(), e);
            std::process::exit(1);
        }
    };

    // Lex
    let lexer = Lexer::new(&source);

    // Parse
    let arena = Bump::new();
    let mut parser = Parser::new(lexer, &arena);
    let program = parser.parse_program();

    if !parser.errors.is_empty() {
        eprintln!("Parser errors:");
        for err in &parser.errors {
            eprintln!("  - {}", err);
        }
        std::process::exit(1);
    }

    // Compile
    let mut compiler = Compiler::new();
    match compiler.compile(&program.statements) {
        Ok(compiled) => {
            let output_path = cli.output.unwrap_or_else(|| {
                cli.input.with_extension("exki")
            });

            let mut file = match fs::File::create(&output_path) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("Error creating {}: {}", output_path.display(), e);
                    std::process::exit(1);
                }
            };

            if let Err(e) = exn::write_exn(&mut file, compiled) {
                eprintln!("Error writing .exki: {}", e);
                std::process::exit(1);
            }

            println!("Compiled successfully: {} -> {}", cli.input.display(), output_path.display());
            println!("  Functions: {}", compiled.functions.len());
            println!("  Main instructions: {}", compiled.main.instructions.len());
        }
        Err(e) => {
            eprintln!("Compilation error: {}", e);
            std::process::exit(1);
        }
    }
}
