/// KiVM CLI - Kinetix Virtual Machine
/// Loads and executes .exki bytecode bundles.

use clap::Parser as ClapParser;
use kinetix_kicomp::exn;
use kinetix_kivm::vm::VM;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

// Magic signature for bundled executables (17 bytes)
const BUNDLE_SIGNATURE: &[u8] = b"KINETIX_BUNDLE_V1";

#[derive(ClapParser)]
#[command(name = "kivm")]
#[command(about = "KiVM — Kinetix bytecode virtual machine")]
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
    },
    /// Show version information
    Version,
    /// Run unit tests in a directory or file
    Test {
        /// Path to the test file or directory
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

fn main() {
    // 1. Check if we are running as a bundled executable
    if let Some(program) = check_for_bundle() {
        // Run the bundled program
        let mut vm = VM::new(program);
        if let Err(e) = vm.run() {
            eprintln!("Runtime Error: {}", e);
            std::process::exit(1);
        }
        return;
    }

    // 2. Otherwise/Normal CLI mode
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
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
    // So reading 8 bytes now.
    // Actually, let's just seek back 8 more bytes.
    // My seek was: End - (17 + 8).
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
            let mut parser = kinetix_language::parser::Parser::new(lexer);
            let ast = parser.parse_program();

            if !parser.errors.is_empty() {
                eprintln!("Parser errors:");
                for e in &parser.errors { eprintln!("  - {}", e); }
                return Err("Parsing failed".into());
            }

            let mut compiler = Compiler::new();
            let compiled = compiler.compile(&ast.statements).map_err(|e| format!("Compilation error: {}", e))?;
            
            let mut vm = VM::new(compiled.clone());
            vm.run().map_err(|e| format!("Runtime error: {}", e))?;
        }
        Commands::Compile { input, output, exe } => {
            let source = fs::read_to_string(&input).map_err(|e| format!("Error reading {}: {}", input.display(), e))?;
            
            // Preprocess includes
            let source = preprocess_includes(&source, input.parent().unwrap_or(Path::new(".")))
                .map_err(|e| format!("Include error: {}", e))?;

            use kinetix_kicomp::compiler::Compiler;

            let lexer = kinetix_language::lexer::Lexer::new(&source);
            let mut parser = kinetix_language::parser::Parser::new(lexer);
            let ast = parser.parse_program();

            if !parser.errors.is_empty() {
                eprintln!("Parser errors:");
                for e in &parser.errors { eprintln!("  - {}", e); }
                return Err("Parsing failed".into());
            }

            let mut compiler = Compiler::new();
            let compiled = compiler.compile(&ast.statements).map_err(|e| format!("Compilation error: {}", e))?;

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
            println!("Kinetix CLI v{} build 4", env!("CARGO_PKG_VERSION"));
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
    let mut parser = kinetix_language::parser::Parser::new(lexer);
    let ast = parser.parse_program();

    if !parser.errors.is_empty() {
        return Err(format!("Parser errors: {:?}", parser.errors));
    }

    // 2. Compiling
    let mut compiler = Compiler::new();
    let compiled = compiler.compile(&ast.statements).map_err(|e| format!("Compilation error: {}", e))?;

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
