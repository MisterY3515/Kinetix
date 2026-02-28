/// KiVM CLI - Kinetix Virtual Machine
/// Loads and executes .exki bytecode bundles.

use clap::Parser as ClapParser;
use kinetix_kicomp::exn;
use kinetix_kivm::vm::VM;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use bumpalo::Bump;

// Magic signature for bundled executables (17 bytes)
const BUNDLE_SIGNATURE: &[u8] = b"KINETIX_BUNDLE_V1";

#[derive(ClapParser)]
#[command(name = "kivm")]
#[command(version)]
#[command(about = "Kinetix Virtual Machine & Compiler")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Run an .exki bytecode file
    Run {
        /// Path to the .exki file
        file: PathBuf,
    },
    /// Compile and run a .kix source file directly
    Exec {
        /// Path to the .kix source file
        file: PathBuf,
    },
    /// Compile a .kix source file to .exki bytecode
    Compile {
        /// Input .kix source file
        #[arg(short, long)]
        input: PathBuf,
        /// Output .exki file (optional)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Create a standalone executable (bundle)
        #[arg(long)]
        exe: bool,
        /// Compile to native machine code (LLVM)
        #[arg(long)]
        native: bool,
    },
    /// Show version information
    Version,
    /// Run unit tests in a directory or file
    Test {
        /// Path to the test file or directory
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Start an interactive Kinetix shell (terminal)
    Shell,
    /// Open the Kinetix documentation in the browser
    #[command(alias = "documentation")]
    Docs,
    /// Uninstall Kinetix from the system
    Uninstall,
    /// Repair or modify the Kinetix installation
    Repair,
}

#[cfg(target_os = "windows")]
fn is_launched_from_explorer() -> bool {
    // If the console process list returns <= 1, it means we're the only process attached to this console.
    // This happens when Windows allocates a fresh conhost.exe for us because we were launched via double-click 
    // from Explorer, rather than from an existing cmd.exe or wt.exe terminal process natively.
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetConsoleProcessList(processList: *mut u32, processCount: u32) -> u32;
    }
    unsafe {
        let mut pids = [0u32; 2];
        let count = GetConsoleProcessList(pids.as_mut_ptr(), 2);
        count <= 1
    }
}

fn fatal_error(msg: &str) {
    // Auto-detect file from message: if first line looks like "path/to/file.kix:" extract it
    let first_line = msg.lines().next().unwrap_or("");
    let trimmed_first = first_line.trim();
    if trimmed_first.ends_with(':')
        && (trimmed_first.contains('/') || trimmed_first.contains('\\') || trimmed_first.contains(".kix"))
    {
        let file_path = &trimmed_first[..trimmed_first.len() - 1];
        // Skip the first line (filename) from the message body
        let rest = msg.lines().skip(1).collect::<Vec<_>>().join("\n");
        fatal_error_in(Some(file_path), &rest);
    } else {
        fatal_error_in(None, msg);
    }
}

fn fatal_error_in(file: Option<&str>, msg: &str) {
    let version = env!("CARGO_PKG_VERSION");
    let build = option_env!("KINETIX_BUILD").unwrap_or("Dev");

    eprintln!();

    // File header (if applicable)
    if let Some(f) = file {
        eprintln!("\x1b[1;37m{}:\x1b[0m", f);
    }

    // Structured error output — C++/Python-style diagnostics
    for line in msg.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            eprintln!();
        } else if trimmed.starts_with("error") || trimmed.starts_with("Error")
            || trimmed.contains("Fatal Error") || trimmed.contains("errors:")
        {
            eprintln!("\x1b[1;31merror\x1b[0m: {}", trimmed);
        } else if trimmed.starts_with("warning") || trimmed.starts_with("Warning") {
            eprintln!("\x1b[1;33mwarning\x1b[0m: {}", trimmed);
        } else if trimmed.starts_with("note") || trimmed.starts_with("Note") {
            eprintln!("\x1b[1;36mnote\x1b[0m: {}", trimmed);
        } else if trimmed.starts_with("--> ") {
            // Pre-formatted location reference
            eprintln!("  \x1b[1;34m{}\x1b[0m", trimmed);
        } else if trimmed.starts_with("- ") {
            // Bullet item — likely an individual error
            let detail = &trimmed[2..];
            // Try to extract line number from "Line N:" pattern
            if let Some(rest) = detail.strip_prefix("Line ") {
                if let Some(colon_pos) = rest.find(':') {
                    let line_no = &rest[..colon_pos];
                    let msg_part = rest[colon_pos + 1..].trim();
                    if let Some(f) = file {
                        eprintln!("\x1b[1;31merror\x1b[0m: {}", msg_part);
                        eprintln!("  \x1b[1;34m--> {}:{}\x1b[0m", f, line_no);
                        
                        if let Ok(line_num) = line_no.parse::<usize>() {
                            if let Ok(source_content) = std::fs::read_to_string(f) {
                                if let Some(source_line) = source_content.lines().nth(line_num.saturating_sub(1)) {
                                    let trimmed_line = source_line.trim_start();
                                    
                                    // Try to find a target word in single quotes (e.g. 'math', 'a')
                                    let mut target_word = None;
                                    if let Some(start) = msg_part.find('\'') {
                                        if let Some(end) = msg_part[start + 1..].find('\'') {
                                            target_word = Some(&msg_part[start + 1 .. start + 1 + end]);
                                        }
                                    }

                                    let mut indent = source_line.len() - trimmed_line.len();
                                    let mut caret_len = trimmed_line.len().max(1);
                                    
                                    if let Some(word) = target_word {
                                        if let Some(pos) = source_line.find(word) {
                                            indent = pos;
                                            caret_len = word.len();
                                        }
                                    }

                                    eprintln!("   \x1b[1;34m|\x1b[0m");
                                    eprintln!("\x1b[1;34m{:>2} |\x1b[0m {}", line_num, source_line);
                                    let carets = "^".repeat(caret_len);
                                    let spaces = " ".repeat(indent);
                                    eprintln!("   \x1b[1;34m|\x1b[0m {}\x1b[1;31m{}\x1b[0m", spaces, carets);
                                }
                            }
                        }
                    } else {
                        eprintln!("\x1b[1;31merror\x1b[0m: {}", msg_part);
                        eprintln!("  \x1b[1;34m--> line {}\x1b[0m", line_no);
                    }
                } else {
                    eprintln!("  {}", trimmed);
                }
            } else {
                eprintln!("  {}", trimmed);
            }
        } else {
            eprintln!("  {}", line);
        }
    }

    // Footer
    eprintln!();
    eprintln!("\x1b[1;31merror[E0000]\x1b[0m: aborting due to previous error(s)");
    eprintln!("\x1b[2m  Kinetix v{} ({})\x1b[0m", version, build);
    eprintln!("\x1b[36m  For more information, open an issue: https://github.com/MisterY3515/Kinetix/issues\x1b[0m");
    eprintln!();

    #[cfg(target_os = "windows")]
    {
        if is_launched_from_explorer() {
            let _ = std::process::Command::new("cmd.exe").arg("/c").arg("pause").status();
        }
    }
    std::process::exit(1);
}

/// Format a compile-pipeline error with filename context.
fn format_pipeline_error(file: &std::path::Path, category: &str, errors: Vec<String>) -> String {
    let fname = file.display().to_string();
    let mut out = format!("{}:\n", fname);
    for e in &errors {
        // Check if error already contains "Line N:" pattern
        let trimmed = e.trim();
        if let Some(rest) = trimmed.strip_prefix("Line ") {
            if let Some(colon_pos) = rest.find(':') {
                let line_no = &rest[..colon_pos];
                let msg = rest[colon_pos + 1..].trim();
                out.push_str(&format!("- Line {}: {}\n", line_no, msg));
                continue;
            }
        }
        out.push_str(&format!("- {}\n", trimmed));
    }
    out.push_str(&format!("{} error(s) in {}", errors.len(), category));
    out
}

fn main() {
    // 1. Check if we are running as a bundled executable
    if let Some(program) = check_for_bundle() {
        // Run the bundled program
        let mut vm = VM::new(program);
        if let Err(e) = vm.run() {
            fatal_error(&format!("Runtime error:\n{}", e));
        }

        #[cfg(target_os = "windows")]
        if is_launched_from_explorer() {
            let _ = std::process::Command::new("cmd.exe").arg("/c").arg("pause").status();
        }
        return;
    }

    // 2. Otherwise/Normal CLI mode
    if let Err(e) = run() {
        fatal_error(&format!("{}", e));
    }

    #[cfg(target_os = "windows")]
    if is_launched_from_explorer() {
        let _ = std::process::Command::new("cmd.exe").arg("/c").arg("pause").status();
    }
}

fn check_for_bundle() -> Option<kinetix_kicomp::ir::CompiledProgram> {
    let current_exe = std::env::current_exe().ok()?;
    let mut file = fs::File::open(&current_exe).ok()?;
    
    // Seek to end - signature length - size length (8 bytes)
    let footer_len = BUNDLE_SIGNATURE.len() as i64 + 8;
    let file_len = file.metadata().ok()?.len();
    if file_len < footer_len as u64 { return None; }

    file.seek(SeekFrom::End(-footer_len)).ok()?;
    
    // Read signature
    let mut sig_buf = vec![0u8; BUNDLE_SIGNATURE.len()];
    file.read_exact(&mut sig_buf).ok()?;
    
    if sig_buf != BUNDLE_SIGNATURE {
        return None;
    }

    // Read payload size (u64) - it's before the signature in my design? 
    // Wait, typical is [Payload] [Size] [Sig].
    // I sought to End - Sig - 8.
    // So current position is at [Size].
    // Let's read size.
    let mut size_buf = [0u8; 8];
    file.read_exact(&mut size_buf).ok()?;
    let payload_size = u64::from_le_bytes(size_buf);

    // Seek to start of payload
    // Position = End - Footer - PayloadSize
    let start_pos = file_len - footer_len as u64 - payload_size;
    file.seek(SeekFrom::Start(start_pos)).ok()?;

    // Read payload
    // We can use read_exn directly from the file stream at this position
    // Assuming read_exn reads exactly what it needs or we limit it.
    // read_exn reads until end of valid bytecode structure.
    // It takes a Read. We can give it the file.
    // But we should probably limit it just in case.
    let mut handle = file.take(payload_size);
    exn::read_exn(&mut handle).ok()
}

fn run() -> Result<(), String> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run { file } => {
            if file.extension().and_then(|s| s.to_str()) == Some("kix") {
                return Err(format!("'{}' is a source file. Use 'kivm compile --run {}' instead.", file.display(), file.display()));
            }

            let data = fs::read(&file).map_err(|e| format!("Error reading {}: {}", file.display(), e))?;
            let mut cursor = std::io::Cursor::new(data);
            let program = exn::read_exn(&mut cursor).map_err(|e| format!("Error loading .exki: {}", e))?;
            let mut vm = VM::new(program);
            vm.run().map_err(|e| format!("Runtime error: {}", e))?;
        }
        Commands::Exec { file } => {
            let source = fs::read_to_string(&file).map_err(|e| format!("Error reading {}: {}", file.display(), e))?;
            use kinetix_kicomp::compiler::Compiler;

            let lexer = kinetix_language::lexer::Lexer::new(&source);
            let arena = Bump::new();
            let mut parser = kinetix_language::parser::Parser::new(lexer, &arena);
            let ast = parser.parse_program();

            if !parser.errors.is_empty() {
                let errs: Vec<String> = parser.errors.iter().map(|e| e.to_string()).collect();
                return Err(format_pipeline_error(&file, "Parser", errs));
            }

            let symbols = kinetix_kicomp::symbol::resolve_program(&ast.statements)
                .map_err(|errs| format_pipeline_error(&file, "Symbol Resolution", errs))?;

            let mut traits = kinetix_kicomp::trait_solver::TraitEnvironment::new();
            for stmt in &ast.statements {
                if let kinetix_language::ast::Statement::Trait { .. } = stmt {
                    if let Err(e) = traits.register_trait(stmt) {
                        return Err(format_pipeline_error(&file, "Trait Resolver", vec![e]));
                    }
                }
            }
            for stmt in &ast.statements {
                if let kinetix_language::ast::Statement::Impl { .. } = stmt {
                    if let Err(e) = traits.register_impl(stmt) {
                        return Err(format_pipeline_error(&file, "Trait Resolver", vec![e]));
                    }
                }
            }
            traits.validate_cycles().map_err(|e| format_pipeline_error(&file, "Trait Resolver", vec![e]))?;

            let mut hir = kinetix_kicomp::hir::lower_to_hir(&ast.statements, &symbols, &traits);
            kinetix_kicomp::type_normalize::normalize(&mut hir, &symbols).map_err(|e| format_pipeline_error(&file, "Type Normalizer", vec![e]))?;
            let mut ctx = kinetix_kicomp::typeck::TypeContext::new();
            let constraints = ctx.collect_constraints(&hir);
            ctx.solve(&constraints).map_err(|errs| {
                let msgs: Vec<String> = errs.iter().map(|e| e.to_string()).collect();
                format_pipeline_error(&file, "Type Checker", msgs)
            })?;

            // Post-TypeChecker: resolve method calls now that types are concrete
            kinetix_kicomp::type_normalize::resolve_method_calls(&mut hir, &symbols, &ctx.substitution)
                .map_err(|e| format_pipeline_error(&file, "Method Resolution", vec![e]))?;

            kinetix_kicomp::exhaustiveness::check_program_exhaustiveness(&hir, &symbols, &ctx.substitution)
                .map_err(|e| format_pipeline_error(&file, "Exhaustiveness Checker", vec![e]))?;

            // M2.6 Capability IR Enforcement Pass (Build 19)
            let granted_caps = vec![
                kinetix_kicomp::capability::Capability::FsRead,
                kinetix_kicomp::capability::Capability::FsWrite,
                kinetix_kicomp::capability::Capability::NetAccess,
                kinetix_kicomp::capability::Capability::SysInfo,
                kinetix_kicomp::capability::Capability::OsExecute,
            ];
            let cap_validator = kinetix_kicomp::capability::CapabilityValidator::new(granted_caps.clone());
            cap_validator.validate(&hir).map_err(|errs| {
                let msgs: Vec<String> = errs.iter().map(|e| e.to_string()).collect();
                format_pipeline_error(&file, "Sandbox Audit Pass", msgs)
            })?;

            let mir = kinetix_kicomp::mir::lower_to_mir(&hir, &ctx.substitution);
            kinetix_kicomp::borrowck::check_mir(&mir).map_err(|errs| {
                format_pipeline_error(&file, "Borrow Checker", errs)
            })?;

            let mir = kinetix_kicomp::monomorphize::monomorphize(&mir).map_err(|e| {
                format_pipeline_error(&file, "Monomorphization Pass", vec![e])
            })?;

            kinetix_kicomp::mono_validate::validate(&mir).map_err(|e| {
                format_pipeline_error(&file, "Post-Mono Validator", vec![e])
            })?;

            kinetix_kicomp::drop_verify::verify(&mir).map_err(|e| {
                format_pipeline_error(&file, "Drop Order Verifier", vec![e])
            })?;

            #[cfg(feature = "llvm")]
            {
                // Try LLVM JIT first
                let jit_res = kinetix_kicomp::llvm_codegen::run_program_jit(&ast.statements);
                match jit_res {
                    Ok(_) => return Ok(()), // JIT successful
                    Err(e) => {
                        // JIT failed (likely due to unimplemented AST nodes in LLVM backend yet).
                        // Fallback to KiVM.
                        // We do not print the fallback message unless in debug mode, to avoid noise,
                        // but since it's experimental, let's log it lightly.
                        // eprintln!("Note: LLVM JIT not available for this script ({}). Falling back to KiVM.", e);
                    }
                }
            }

            let reactive_graph = kinetix_kicomp::reactive::build_reactive_graph(&hir)
                .map_err(|e| format!("Reactive Graph Error: {}", e))?;
                
            let mut compiler = Compiler::new();
            let compiled = compiler.compile(&ast.statements, Some(reactive_graph.to_compiled())).map_err(|e| format!("Compilation error: {}", e))?;
            
            let mut vm = VM::new(compiled.clone());
            vm.run().map_err(|e| format!("Runtime error: {}", e))?;
        }
        Commands::Compile { input, output, exe, native } => {
            let source = fs::read_to_string(&input).map_err(|e| format!("Error reading {}: {}", input.display(), e))?;
            
            // Preprocess includes
            let source = preprocess_includes(&source, input.parent().unwrap_or(Path::new(".")))
                .map_err(|e| format!("Include error: {}", e))?;

            use kinetix_kicomp::compiler::Compiler;

            let lexer = kinetix_language::lexer::Lexer::new(&source);
            let arena = Bump::new();
            let mut parser = kinetix_language::parser::Parser::new(lexer, &arena);
            let ast = parser.parse_program();

            if !parser.errors.is_empty() {
                let errs: Vec<String> = parser.errors.iter().map(|e| e.to_string()).collect();
                return Err(format_pipeline_error(&input, "Parser", errs));
            }

            let symbols = kinetix_kicomp::symbol::resolve_program(&ast.statements)
                .map_err(|errs| format_pipeline_error(&input, "Symbol Resolution", errs))?;

            let mut traits = kinetix_kicomp::trait_solver::TraitEnvironment::new();
            for stmt in &ast.statements {
                if let kinetix_language::ast::Statement::Trait { .. } = stmt {
                    if let Err(e) = traits.register_trait(stmt) {
                        return Err(format_pipeline_error(&input, "Trait Resolver", vec![e]));
                    }
                }
            }
            for stmt in &ast.statements {
                if let kinetix_language::ast::Statement::Impl { .. } = stmt {
                    if let Err(e) = traits.register_impl(stmt) {
                        return Err(format_pipeline_error(&input, "Trait Resolver", vec![e]));
                    }
                }
            }
            traits.validate_cycles().map_err(|e| format_pipeline_error(&input, "Trait Resolver", vec![e]))?;

            let mut hir = kinetix_kicomp::hir::lower_to_hir(&ast.statements, &symbols, &traits);
            kinetix_kicomp::type_normalize::normalize(&mut hir, &symbols).map_err(|e| format_pipeline_error(&input, "Type Normalizer", vec![e]))?;
            let mut ctx = kinetix_kicomp::typeck::TypeContext::new();
            let constraints = ctx.collect_constraints(&hir);
            ctx.solve(&constraints).map_err(|errs| {
                let msgs: Vec<String> = errs.iter().map(|e| e.to_string()).collect();
                format_pipeline_error(&input, "Type Checker", msgs)
            })?;

            // Post-TypeChecker: resolve method calls now that types are concrete
            kinetix_kicomp::type_normalize::resolve_method_calls(&mut hir, &symbols, &ctx.substitution)
                .map_err(|e| format_pipeline_error(&input, "Method Resolution", vec![e]))?;

            kinetix_kicomp::exhaustiveness::check_program_exhaustiveness(&hir, &symbols, &ctx.substitution)
                .map_err(|e| format_pipeline_error(&input, "Exhaustiveness Checker", vec![e]))?;

            // M2.6 Capability IR Enforcement Pass (Build 19)
            let cap_validator = kinetix_kicomp::capability::CapabilityValidator::new(vec![
                kinetix_kicomp::capability::Capability::FsRead,
                kinetix_kicomp::capability::Capability::FsWrite,
                kinetix_kicomp::capability::Capability::NetAccess,
                kinetix_kicomp::capability::Capability::SysInfo,
                kinetix_kicomp::capability::Capability::OsExecute,
            ]);
            cap_validator.validate(&hir).map_err(|errs| {
                let msgs: Vec<String> = errs.iter().map(|e| e.to_string()).collect();
                format_pipeline_error(&input, "Sandbox Audit Pass", msgs)
            })?;

            let mir = kinetix_kicomp::mir::lower_to_mir(&hir, &ctx.substitution);
            kinetix_kicomp::borrowck::check_mir(&mir).map_err(|errs| {
                format_pipeline_error(&input, "Borrow Checker", errs)
            })?;

            let mir = kinetix_kicomp::monomorphize::monomorphize(&mir).map_err(|e| {
                format_pipeline_error(&input, "Monomorphization Pass", vec![e])
            })?;

            kinetix_kicomp::mono_validate::validate(&mir).map_err(|e| {
                format_pipeline_error(&input, "Post-Mono Validator", vec![e])
            })?;

            kinetix_kicomp::drop_verify::verify(&mir).map_err(|e| {
                format_pipeline_error(&input, "Drop Order Verifier", vec![e])
            })?;

            let reactive_graph = kinetix_kicomp::reactive::build_reactive_graph(&hir)
                .map_err(|e| format!("Reactive Graph Error: {}", e))?;

            let mut compiler = Compiler::new();
            let compiled = compiler.compile(&ast.statements, Some(reactive_graph.to_compiled()))
                .map_err(|e| format!("Compilation error: {}", e))?;

            if native {
                #[cfg(feature = "llvm")]
                {
                    let output_path = output.unwrap_or_else(|| {
                        // Default to .o for native object files
                        input.with_extension("o")
                    });
                    
                    println!("Compiling to native object file: {}", output_path.display());
                    kinetix_kicomp::llvm_codegen::compile_program_to_object(&ast.statements, &output_path)
                        .map_err(|e| format!("LLVM Codegen error: {}", e))?;
                        
                    println!("Native compilation successful.");
                    return Ok(());
                }
                #[cfg(not(feature = "llvm"))]
                {
                   return Err("Native compilation requires 'llvm' feature. Rebuild with --features llvm.".to_string());
                }
            }

            if exe {
                // Create standalone executable
                let output_path = output.unwrap_or_else(|| {
                    if cfg!(target_os = "windows") {
                        input.with_extension("exe")
                    } else {
                        // On Unix, strip extension to produce a clean binary name
                        input.with_extension("")
                    }
                });

                // 1. Serialize bytecode to buffer
                let mut bytecode_buf = Vec::new();
                exn::write_exn(&mut bytecode_buf, compiled).map_err(|e| e.to_string())?;
                let payload_size = bytecode_buf.len() as u64;

                // 2. Read current executable (the stub)
                let current_exe = std::env::current_exe().map_err(|e| e.to_string())?;
                let mut stub_data = Vec::new();
                fs::File::open(&current_exe).map_err(|e| e.to_string())?
                    .read_to_end(&mut stub_data).map_err(|e| e.to_string())?;

                // 3. Write [Stub] [Payload] [Size] [Sig]
                let mut file = fs::File::create(&output_path).map_err(|e| format!("Error creating {}: {}", output_path.display(), e))?;
                
                file.write_all(&stub_data).map_err(|e| e.to_string())?;
                file.write_all(&bytecode_buf).map_err(|e| e.to_string())?;
                file.write_all(&payload_size.to_le_bytes()).map_err(|e| e.to_string())?;
                file.write_all(BUNDLE_SIGNATURE).map_err(|e| e.to_string())?;

                // 4. Make executable (Linux/Mac)
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mut perms = fs::metadata(&output_path).unwrap().permissions();
                    perms.set_mode(0o755);
                    fs::set_permissions(&output_path, perms).unwrap();
                }

                println!("Bundle created successfully: {}", output_path.display());

            } else {
                // Normal .exki compilation
                let output_path = output.unwrap_or_else(|| {
                    input.with_extension("exki")
                });

                let mut file = fs::File::create(&output_path).map_err(|e| format!("Error creating {}: {}", output_path.display(), e))?;
                exn::write_exn(&mut file, compiled).map_err(|e| format!("Error writing .exki: {}", e))?;
                println!("Compiled successfully: {} -> {}", input.display(), output_path.display());
            }
        }
        Commands::Version => {
            println!("  Kinetix v{} ({})", env!("CARGO_PKG_VERSION"), kinetix_kicomp::compiler::CURRENT_BUILD);
        }
        Commands::Shell => {
            run_shell();
        }
        Commands::Docs => {
            open_docs()?;
        }
        Commands::Uninstall => {
            open_installer("--uninstall")?;
        }
        Commands::Repair => {
            open_installer("--repair")?;
        }
        Commands::Test { path } => {
             let mut passed = 0;
             let mut failed = 0;
             let start_time = std::time::Instant::now();

             run_tests_recursive(&path, &mut passed, &mut failed)?;

             let duration = start_time.elapsed();
             println!("\nTest Summary:");
             println!("  Passed: {}", passed);
             println!("  Failed: {}", failed);
             println!("  Time:   {:.2?}", duration);
             
             if failed > 0 {
                 std::process::exit(1);
             }
        }
    }

    Ok(())
}

fn run_tests_recursive(path: &Path, passed: &mut usize, failed: &mut usize) -> Result<(), String> {
    if path.is_dir() {
        for entry in fs::read_dir(path).map_err(|e| format!("Error reading dir {}: {}", path.display(), e))? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.is_dir() {
                run_tests_recursive(&path, passed, failed)?;
            } else {
                run_tests_recursive(&path, passed, failed)?;
            }
        }
    } else if let Some(ext) = path.extension() {
        if ext == "kix" && path.file_name().unwrap().to_str().unwrap().starts_with("test_") {
             print!("Running {} ... ", path.display());
             std::io::stdout().flush().unwrap();

             match run_test_file(path) {
                 Ok(_) => {
                     println!("OK");
                     *passed += 1;
                 },
                 Err(e) => {
                     println!("FAILED");
                     println!("  Error: {}", e);
                     *failed += 1;
                 }
             }
        }
    }
    Ok(())
}

fn run_test_file(path: &Path) -> Result<(), String> {
    let source = fs::read_to_string(path).map_err(|e| e.to_string())?;
    
    // Preprocess includes
    let source = preprocess_includes(&source, path.parent().unwrap_or(Path::new(".")))?;

    use kinetix_kicomp::compiler::Compiler;

    // 1. Lexing
    let lexer = kinetix_language::lexer::Lexer::new(&source);
    let arena = Bump::new();
    let mut parser = kinetix_language::parser::Parser::new(lexer, &arena);
    let ast = parser.parse_program();

    if !parser.errors.is_empty() {
        return Err(format!("Parser errors: {:?}", parser.errors));
    }

    let symbols = kinetix_kicomp::symbol::resolve_program(&ast.statements)
        .map_err(|errs| format!("Symbol errors: {:?}", errs))?;
    let traits = kinetix_kicomp::trait_solver::TraitEnvironment::new();
    let hir = kinetix_kicomp::hir::lower_to_hir(&ast.statements, &symbols, &traits);
    let reactive_graph = kinetix_kicomp::reactive::build_reactive_graph(&hir)
        .map_err(|e| format!("Reactive Graph Error: {}", e))?;

    let mut compiler = Compiler::new();
    let compiled = compiler.compile(&ast.statements, Some(reactive_graph.to_compiled()))
        .map_err(|e| format!("Compilation error: {}", e))?;

    // 3. Running
    let mut vm = VM::new(compiled.clone());
    if let Err(e) = vm.run() {
        return Err(format!("Runtime error: {}", e));
    }
    Ok(())
}

fn preprocess_includes(source: &str, base_path: &Path) -> Result<String, String> {
    let mut result = String::new();
    for line in source.lines() {
        if line.trim().starts_with("#include") {
            // Parse path: #include "path/to/file.kix"
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let path_str = parts[1].trim_matches('"');
                let include_path = base_path.join(path_str);
                
                if include_path.exists() {
                     let included_source = fs::read_to_string(&include_path)
                        .map_err(|e| format!("Failed to read include {}: {}", include_path.display(), e))?;
                     // Recursive include
                     let processed = preprocess_includes(&included_source, include_path.parent().unwrap_or(Path::new(".")))?;
                     result.push_str(&processed);
                     result.push('\n');
                } else {
                    return Err(format!("Include not found: {}", include_path.display()));
                }
            } else {
                 return Err("Invalid include syntax".to_string());
            }
        } else {
            result.push_str(line);
            result.push('\n');
        }
    }
    Ok(result)
}

/// Interactive Kinetix Shell — a terminal REPL with bash-like commands + Kinetix expressions.
fn run_shell() {
    use kinetix_kicomp::compiler::Compiler;

    let build = option_env!("KINETIX_BUILD").unwrap_or("Dev");
    println!("\x1b[1;35mKinetix Shell\x1b[0m v{} ({})", env!("CARGO_PKG_VERSION"), build);
    println!("Type \x1b[36mexit\x1b[0m to quit, \x1b[36mhelp\x1b[0m for commands.\n");

    let mut line_buf = String::new();

    loop {
        // Prompt: show current dir
        let cwd = std::env::current_dir()
            .map(|p| {
                let s = p.to_string_lossy().to_string();
                // Shorten home dir
                if let Ok(home) = std::env::var("HOME").or(std::env::var("USERPROFILE")) {
                    if s.starts_with(&home) {
                        return format!("~{}", &s[home.len()..]);
                    }
                }
                s
            })
            .unwrap_or_else(|_| "?".into());

        print!("\x1b[1;34m{}\x1b[0m \x1b[1;33m❯\x1b[0m ", cwd);
        std::io::stdout().flush().ok();

        line_buf.clear();
        if std::io::stdin().read_line(&mut line_buf).is_err() {
            break;
        }

        let input = line_buf.trim();
        if input.is_empty() { continue; }

        // Exit
        if input == "exit" || input == "quit" {
            println!("Bye!");
            break;
        }

        // Help
        if input == "help" {
            println!("\x1b[1mBuilt-in commands:\x1b[0m");
            println!("  ls [dir]          List directory contents");
            println!("  cd [dir]          Change directory");
            println!("  pwd               Print working directory");
            println!("  cat <file>        Show file contents");
            println!("  head <file> [n]   Show first n lines");
            println!("  tail <file> [n]   Show last n lines");
            println!("  mkdir <dir>       Create directory");
            println!("  touch <file>      Create empty file");
            println!("  rm <path>         Remove file or directory");
            println!("  cp <src> <dst>    Copy file");
            println!("  mv <src> <dst>    Move/rename file");
            println!("  echo <text>       Print text");
            println!("  grep <pat> <file> Search in file");
            println!("  wc <file>         Word/line/byte count");
            println!("  which <cmd>       Find command in PATH");
            println!("  whoami            Current user name");
            println!("  clear             Clear screen");
            println!("  env               Show environment variables");
            println!("  kivm <args>       Run kivm subcommands");
            println!("  exit              Exit shell");
            println!("\n  Anything else is evaluated as a Kinetix expression.");
            continue;
        }

        // Parse command and arguments
        let parts: Vec<&str> = input.split_whitespace().collect();
        let cmd = parts[0];
        let cmd_args: Vec<kinetix_kivm::vm::Value> = parts[1..]
            .iter()
            .map(|s| kinetix_kivm::vm::Value::Str(s.to_string()))
            .collect();

        // Try bash-like commands first
        match cmd {
            "ls" | "cd" | "pwd" | "cat" | "mkdir" | "rm" | "cp" | "mv" |
            "echo" | "touch" | "which" | "whoami" | "clear" | "env" |
            "head" | "tail" | "wc" | "grep" => {
                match kinetix_kivm::builtins::modules::term::call(cmd, &cmd_args) {
                    Ok(kinetix_kivm::vm::Value::Null) => {},
                    Ok(val) => println!("{}", val),
                    Err(e) => eprintln!("\x1b[31m{}\x1b[0m", e),
                }
            }
            "kivm" => {
                // Forward to a child process
                let child_args: Vec<&str> = parts[1..].to_vec();
                let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("kivm"));
                match std::process::Command::new(&exe).args(&child_args).status() {
                    Ok(status) => {
                        if !status.success() {
                            if let Some(code) = status.code() {
                                eprintln!("kivm exited with code {}", code);
                            }
                        }
                    }
                    Err(e) => eprintln!("\x1b[31mFailed to run kivm: {}\x1b[0m", e),
                }
            }
            _ => {
                // Try to evaluate as Kinetix source
                let source = input.to_string();
                let lexer = kinetix_language::lexer::Lexer::new(&source);
                let arena = Bump::new();
                let mut parser = kinetix_language::parser::Parser::new(lexer, &arena);
                let ast = parser.parse_program();

                if !parser.errors.is_empty() {
                    // Not valid Kinetix — try as system command
                    match std::process::Command::new(cmd)
                        .args(&parts[1..])
                        .status()
                    {
                        Ok(status) => {
                            if !status.success() {
                                if let Some(code) = status.code() {
                                    eprintln!("Process exited with code {}", code);
                                }
                            }
                        }
                        Err(_) => {
                            eprintln!("\x1b[31mUnknown command: {}\x1b[0m", cmd);
                        }
                    }
                } else {
                    let mut compiler = Compiler::new();
                    match compiler.compile(&ast.statements, None) {
                        Ok(compiled) => {
                            let mut vm = VM::new(compiled.clone());
                            if let Err(e) = vm.run() {
                                eprintln!("\x1b[31mRuntime error: {}\x1b[0m", e);
                            }
                        }
                        Err(e) => eprintln!("\x1b[31mCompilation error: {}\x1b[0m", e),
                    }
                }
            }
        }
    }
}

/// Open the installed documentation in the default browser.
fn open_docs() -> Result<(), String> {
    let docs_path = if let Some(dirs) = directories::BaseDirs::new() {
        dirs.home_dir().join(".kinetix").join("docs").join("index.html")
    } else {
        return Err("Cannot determine home directory".into());
    };

    if !docs_path.exists() {
        return Err(format!(
            "Documentation not found at {}.\nInstall it via the Kinetix Installer (enable 'Documentation').",
            docs_path.display()
        ));
    }

    // Open in default browser
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &docs_path.to_string_lossy()])
            .spawn()
            .map_err(|e| format!("Failed to open browser: {}", e))?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&docs_path)
            .spawn()
            .map_err(|e| format!("Failed to open browser: {}", e))?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&docs_path)
            .spawn()
            .map_err(|e| format!("Failed to open browser: {}", e))?;
    }

    println!("Opening documentation: {}", docs_path.display());
    Ok(())
}

fn open_installer(arg: &str) -> Result<(), String> {
    let exe_path = std::env::current_exe().map_err(|e| e.to_string())?;
    
    // The installer is usually parallel to `kivm.exe` in `target/debug` or `dist/`
    let mut installer_path = exe_path.parent()
        .map(|p| p.join("installer.exe"))
        .unwrap_or_else(|| PathBuf::from("installer.exe"));

    if !installer_path.exists() {
        // Fallback for development (e.g., if we're in `target/debug` but we want `installer.exe`)
        installer_path = PathBuf::from("installer.exe");
    }

    std::process::Command::new(&installer_path)
        .arg(arg)
        .spawn()
        .map_err(|e| format!("Failed to spawn installer: {}", e))?;
    Ok(())
}
