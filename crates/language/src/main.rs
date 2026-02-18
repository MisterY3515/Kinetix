mod lexer;
mod ast;
mod parser;

use clap::Parser;
use std::fs;
use bumpalo::Bump;
use lexer::Lexer;
use parser::Parser as NevharParser; // Avoid name collision

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    input: Option<String>,
}

fn main() {
    let args = Args::parse();

    if let Some(path) = args.input {
        match fs::read_to_string(&path) {
            Ok(content) => {
                let arena = Bump::new();
                let l = Lexer::new(&content);
                let mut p = NevharParser::new(l, &arena);
                let program = p.parse_program();
                
                println!("Parsed Program: {:?}", program);
                
                if !p.errors.is_empty() {
                    eprintln!("Parser Errors:");
                    for msg in p.errors {
                        eprintln!("\t{}", msg);
                    }
                }
            }
            Err(e) => eprintln!("Error reading file: {}", e),
        }
    } else {
        println!("Nevhar Parser Test");
    }
}

