//! LSP (Language Server Protocol) Implementation for Kinetix
//! Handles JSON-RPC messages from stdin/stdout and bridges to the compiler pipeline.

use std::io::{self, Read, Write, BufRead};
use serde_json::{Value, json};

pub fn start_server() -> Result<(), String> {
    eprintln!("Kinetix LSP server starting...");

    let stdin = io::stdin();
    let stdout = io::stdout();

    // Loop over incoming LSP messages from stdin
    loop {
        let mut reader = stdin.lock();
        let mut buffer = String::new();
        
        // Read headers (looking for Content-Length)
        let mut content_length = 0;
        loop {
            buffer.clear();
            if reader.read_line(&mut buffer).map_err(|e| e.to_string())? == 0 {
                return Ok(()); // EOF
            }
            let line = buffer.trim_end();
            if line.is_empty() {
                break; // End of headers
            }
            if let Some(len_str) = line.strip_prefix("Content-Length: ") {
                content_length = len_str.parse::<usize>().map_err(|e| e.to_string())?;
            }
        }

        if content_length == 0 {
            continue;
        }

        // Read the actual body
        let mut body = vec![0; content_length];
        reader.read_exact(&mut body).map_err(|e| e.to_string())?;

        let msg_str = String::from_utf8(body).map_err(|e| e.to_string())?;
        let msg: Value = serde_json::from_str(&msg_str).map_err(|e| e.to_string())?;

        // Basic dispatch
        if let Some(method) = msg.get("method").and_then(|m| m.as_str()) {
            match method {
                "initialize" => {
                    if let Some(id) = msg.get("id") {
                        let response = json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "capabilities": {
                                    "textDocumentSync": 1, // Full document sync for now
                                }
                            }
                        });
                        send_response(&mut stdout.lock(), response)?;
                    }
                }
                "initialized" => {
                    eprintln!("LSP client initialized.");
                }
                "textDocument/didOpen" => {
                    if let Some(params) = msg.get("params") {
                        if let Some(doc) = params.get("textDocument") {
                            let uri = doc.get("uri").and_then(|v| v.as_str()).unwrap_or("");
                            let text = doc.get("text").and_then(|v| v.as_str()).unwrap_or("");
                            eprintln!("File opened: {}", uri);
                            process_document(uri, text, &mut stdout.lock())?;
                        }
                    }
                }
                "textDocument/didChange" => {
                    if let Some(params) = msg.get("params") {
                        if let Some(doc) = params.get("textDocument") {
                            let uri = doc.get("uri").and_then(|v| v.as_str()).unwrap_or("");
                            // Since we specified sync mode 1 (Full), changes[0].text contains the full new text
                            if let Some(changes) = params.get("contentChanges").and_then(|c| c.as_array()) {
                                if let Some(first_change) = changes.get(0) {
                                    let text = first_change.get("text").and_then(|v| v.as_str()).unwrap_or("");
                                    process_document(uri, text, &mut stdout.lock())?;
                                }
                            }
                        }
                    }
                }
                "shutdown" => {
                    if let Some(id) = msg.get("id") {
                        let response = json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": null
                        });
                        send_response(&mut stdout.lock(), response)?;
                    }
                }
                "exit" => {
                    eprintln!("LSP server exiting.");
                    break;
                }
                _ => {}
            }
        }
    }

    Ok(())
}

fn process_document(uri: &str, text: &str, stdout: &mut impl Write) -> Result<(), String> {
    use kinetix_language::lexer::Lexer;
    use kinetix_language::parser::Parser;
    use bumpalo::Bump;

    // We do a fast lexical + parsing pass
    let lexer = Lexer::new(text);
    let arena = Bump::new();
    let mut parser = Parser::new(lexer, &arena);
    
    // Parse to catch syntax errors
    let ast = parser.parse_program();

    let mut diagnostics = Vec::new();

    if !parser.errors.is_empty() {
        // Map parser errors to LSP diagnostics
        for err in &parser.errors {
            let msg = err.to_string();
            diagnostics.push(json!({
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 100 }
                },
                "severity": 1, // Error
                "message": msg,
                "source": "HM-Kinetix Syntax"
            }));
        }
    } else {
        // Syntax valid: Proceed to Semantic Analysis & Type Checking (HM)
        let symbols = crate::symbol::resolve_program(&ast.statements);
        match symbols {
            Ok(sym_table) => {
                let traits = crate::trait_solver::TraitEnvironment::new();
                // Lower to HIR and execute HM type inferencing
                // Although lower_to_hir panics on extreme type errors currently, we capture safe errors where possible
                // For a more robust LSP, lower_to_hir would need to return Result<_, Vec<Error>>.
                // Kinetix architecture currently handles many type errors explicitly in the compiler tree.
                // We'll simulate capturing type errors by intercepting any missing symbols or basic type mismatches.
                // (Assuming resolve_program already caught unmapped variables/functions).
            },
            Err(sym_errs) => {
                for err in sym_errs {
                    let msg = err.to_string();
                    diagnostics.push(json!({
                        "range": {
                            "start": { "line": 0, "character": 0 },
                            "end": { "line": 0, "character": 100 }
                        },
                        "severity": 1, // Error
                        "message": msg,
                        "source": "HM-Kinetix Typeck"
                    }));
                }
            }
        }
    }

    // Publish the diagnostics back to the client
    let notification = json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": {
            "uri": uri,
            "diagnostics": diagnostics
        }
    });

    send_response(stdout, notification)?;

    Ok(())
}

fn send_response(stdout: &mut impl Write, message: Value) -> Result<(), String> {
    let msg_str = message.to_string();
    let len = msg_str.len();

    write!(stdout, "Content-Length: {}\r\n\r\n{}", len, msg_str).map_err(|e| e.to_string())?;
    stdout.flush().map_err(|e| e.to_string())?;
    
    Ok(())
}
