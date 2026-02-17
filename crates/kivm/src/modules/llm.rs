use crate::vm::{Value, VM};
use serde_json::json;

pub fn call(name: &str, args: &[Value], _vm: &mut VM) -> Result<Value, String> {
    match name {
        "ask" | "complete" => {
             let prompt = args.get(0).and_then(|v| match v { Value::Str(s) => Some(s), _ => None }).ok_or("Expected prompt string")?;
             
             // Check optional second arg for options? For now default.
             // Default to generic OpenAI/Ollama style API or just specific one?
             // Prompt implies: User wants an answer.
             
             // Using Ollama localhost default for now as it's free and local.
             // POST http://localhost:11434/api/generate
             let body = json!({
                 "model": "llama3:latest", // Default model, should be configurable!
                 "prompt": prompt,
                 "stream": false
             });
             
             let res = ureq::post("http://localhost:11434/api/generate")
                 .send_json(body)
                 .map_err(|e| format!("Request failed: {}", e))?;
                 
             if res.status() != 200 { return Err(format!("LLM Error: {}", res.status())); }
             
             let json: serde_json::Value = res.into_json().map_err(|e| e.to_string())?;
             let response = json["response"].as_str().unwrap_or("").to_string();
             Ok(Value::Str(response))
        },
        _ => Err(format!("Unknown LLM function: {}", name))
    }
}
