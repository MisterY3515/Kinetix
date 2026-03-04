use crate::vm::Value;
use std::fs;
use std::path::Path;

pub fn call(name: &str, args: &[Value]) -> Result<Value, String> {
    match name {
        // --- File IO (data.file.*) & Path Traversal Security ---
        s if s.starts_with("file.") => {
            let sub_cmd = s.strip_prefix("file.").unwrap();
            
            // Helper for Path Sandbox validation
            fn sanitize_path(input_path: &str) -> Result<std::path::PathBuf, String> {
                let path = Path::new(input_path);
                
                // Block explicit traversal attempts
                for component in path.components() {
                    match component {
                        std::path::Component::ParentDir => return Err("Security Error: Path traversal ('..') is strictly forbidden.".to_string()),
                        std::path::Component::RootDir | std::path::Component::Prefix(_) => return Err("Security Error: Absolute paths are forbidden. Use paths relative to the working directory.".to_string()),
                        _ => {}
                    }
                }
                
                // Allow only sanitized relative paths
                let cwd = std::env::current_dir().map_err(|e| format!("Cannot read cwd: {}", e))?;
                let resolved = cwd.join(path);
                
                // Final safety check: ensuring the resolved path starts with the CWD
                if !resolved.starts_with(&cwd) {
                    return Err("Security Error: Path escapes the current working directory boundary.".to_string());
                }
                
                Ok(resolved)
            }

            match sub_cmd {
                "read" => { // data.file.read(path) -> Result<Map>
                    let path_str = args.first().and_then(|v| match v { Value::Str(s) => Some(s), _ => None }).ok_or("Expected path string")?;
                    let safe_path = sanitize_path(path_str)?;
                    match fs::read_to_string(&safe_path) {
                        Ok(content) => {
                            let mut res = std::collections::HashMap::new();
                            res.insert("ok".to_string(), Value::Str(content));
                            Ok(Value::Map(res))
                        },
                        Err(e) => {
                            let mut res = std::collections::HashMap::new();
                            res.insert("err".to_string(), Value::Str(e.to_string()));
                            Ok(Value::Map(res))
                        }
                    }
                },
                "write" => { // data.file.write(path, content) -> Result<Map>
                    let path_str = args.first().and_then(|v| match v { Value::Str(s) => Some(s), _ => None }).ok_or("Expected path string")?;
                    let content = args.get(1).and_then(|v| match v { Value::Str(s) => Some(s), _ => None }).ok_or("Expected content string")?;
                    let safe_path = sanitize_path(path_str)?;
                    match fs::write(&safe_path, content) {
                        Ok(_) => {
                            let mut res = std::collections::HashMap::new();
                            res.insert("ok".to_string(), Value::Null);
                            Ok(Value::Map(res))
                        },
                        Err(e) => {
                            let mut res = std::collections::HashMap::new();
                            res.insert("err".to_string(), Value::Str(e.to_string()));
                            Ok(Value::Map(res))
                        }
                    }
                },
                "exists" => { // data.file.exists(path) -> bool
                    let path_str = args.first().and_then(|v| match v { Value::Str(s) => Some(s), _ => None }).ok_or("Expected path string")?;
                    // Even `exists` gets sanitized so we don't leak information about outside files
                    match sanitize_path(path_str) {
                        Ok(safe_path) => Ok(Value::Bool(safe_path.exists())),
                        Err(_) => Ok(Value::Bool(false)) // Treat invalid paths as non-existent securely
                    }
                },
                "delete" => { // data.file.delete(path) -> Result<Map>
                    let path_str = args.first().and_then(|v| match v { Value::Str(s) => Some(s), _ => None }).ok_or("Expected path string")?;
                    let safe_path = sanitize_path(path_str)?;
                    match fs::remove_file(&safe_path) {
                        Ok(_) => {
                            let mut res = std::collections::HashMap::new();
                            res.insert("ok".to_string(), Value::Null);
                            Ok(Value::Map(res))
                        },
                        Err(e) => {
                            let mut res = std::collections::HashMap::new();
                            res.insert("err".to_string(), Value::Str(e.to_string()));
                            Ok(Value::Map(res))
                        }
                    }
                },
                "copy" => { // data.file.copy(from, to) -> Result<Map>
                    let src_str = args.first().and_then(|v| match v { Value::Str(s) => Some(s), _ => None }).ok_or("Expected src path string")?;
                    let dst_str = args.get(1).and_then(|v| match v { Value::Str(s) => Some(s), _ => None }).ok_or("Expected dst path string")?;
                    let safe_src = sanitize_path(src_str)?;
                    let safe_dst = sanitize_path(dst_str)?;
                    match fs::copy(&safe_src, &safe_dst) {
                        Ok(_) => {
                            let mut res = std::collections::HashMap::new();
                            res.insert("ok".to_string(), Value::Null);
                            Ok(Value::Map(res))
                        },
                        Err(e) => {
                            let mut res = std::collections::HashMap::new();
                            res.insert("err".to_string(), Value::Str(e.to_string()));
                            Ok(Value::Map(res))
                        }
                    }
                },
                "move" => { // data.file.move(from, to) -> Result<Map>
                    let src_str = args.first().and_then(|v| match v { Value::Str(s) => Some(s), _ => None }).ok_or("Expected src path string")?;
                    let dst_str = args.get(1).and_then(|v| match v { Value::Str(s) => Some(s), _ => None }).ok_or("Expected dst path string")?;
                    let safe_src = sanitize_path(src_str)?;
                    let safe_dst = sanitize_path(dst_str)?;
                    match fs::rename(&safe_src, &safe_dst) {
                        Ok(_) => {
                            let mut res = std::collections::HashMap::new();
                            res.insert("ok".to_string(), Value::Null);
                            Ok(Value::Map(res))
                        },
                        Err(e) => {
                            let mut res = std::collections::HashMap::new();
                            res.insert("err".to_string(), Value::Str(e.to_string()));
                            Ok(Value::Map(res))
                        }
                    }
                },
                _ => Err(format!("Unknown data.file function: {}", sub_cmd))
            }
        },


        // --- Legacy fallback ---
        "read_bytes" => {
            let path = args.first().and_then(|v| match v { Value::Str(s) => Some(s), _ => None }).ok_or("Expected path string")?;
            let bytes = fs::read(path).map_err(|e| e.to_string())?;
            let arr = bytes.into_iter().map(|b| Value::Int(b as i64)).collect();
            Ok(Value::Array(arr))
        },
        "list_dir" => {
            let path = args.first().and_then(|v| match v { Value::Str(s) => Some(s), _ => None }).ok_or("Expected path string")?;
            let entries = fs::read_dir(path).map_err(|e| e.to_string())?;
            let mut list = Vec::new();
            for entry in entries {
                let entry = entry.map_err(|e| e.to_string())?;
                list.push(Value::Str(entry.path().to_string_lossy().to_string()));
            }
            Ok(Value::Array(list))
        },
        "alloc" => {
             let size = args.first().and_then(|v| v.as_int().ok()).ok_or("Expected size int")?;
             let arr = vec![Value::Int(0); size as usize];
             Ok(Value::Array(arr))
        },

        // --- JSON ---
        "json.parse" => {
            let json_str = args.first().and_then(|v| match v { Value::Str(s) => Some(s), _ => None }).ok_or("Expected JSON string")?;
            let v: serde_json::Value = serde_json::from_str(json_str).map_err(|e| e.to_string())?;
            convert_json_to_value(v)
        },
        "json.stringify" => {
             let val = args.first().ok_or("Expected value to stringify")?;
             let json_val = convert_value_to_json(val);
             Ok(Value::Str(json_val.to_string()))
        },

        // --- CSV ---
        "csv.parse" => {
             let csv_str = args.first().and_then(|v| match v { Value::Str(s) => Some(s), _ => None }).ok_or("Expected CSV string")?;
             // Simple naive parse or use crate
             let mut rdr = csv::Reader::from_reader(csv_str.as_bytes());
             let mut rows = Vec::new();
             for result in rdr.records() {
                 let record = result.map_err(|e| e.to_string())?;
                 let row: Vec<Value> = record.iter().map(|s| Value::Str(s.to_string())).collect();
                 rows.push(Value::Array(row));
             }
             Ok(Value::Array(rows))
        },
        "csv.write" => {
             let path = args.first().and_then(|v| match v { Value::Str(s) => Some(s), _ => None }).ok_or("Expected path")?;
             let rows = args.get(1).and_then(|v| match v { Value::Array(a) => Some(a), _ => None }).ok_or("Expected array of rows")?;
             
             let mut wtr = csv::Writer::from_path(path).map_err(|e| e.to_string())?;
             for row_val in rows {
                 if let Value::Array(cols) = row_val {
                     let record: Vec<String> = cols.iter().map(|v| format!("{}", v)).collect();
                     wtr.write_record(&record).map_err(|e| e.to_string())?;
                 }
             }
             wtr.flush().map_err(|e| e.to_string())?;
             Ok(Value::Null)
        },


        _ => Err(format!("Unknown data function: {}", name))
    }
}

fn convert_json_to_value(v: serde_json::Value) -> Result<Value, String> {
    match v {
        serde_json::Value::Null => Ok(Value::Null),
        serde_json::Value::Bool(b) => Ok(Value::Bool(b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() { 
                Ok(Value::Int(i)) 
            } else if let Some(f) = n.as_f64() { 
                Ok(Value::Float(f)) 
            } else {
                Err("JSON number conversion overflow".to_string())
            }
        },
        serde_json::Value::String(s) => Ok(Value::Str(s)),
        serde_json::Value::Array(a) => {
            let list: Result<Vec<Value>, String> = a.into_iter().map(convert_json_to_value).collect();
            Ok(Value::Array(list?))
        },
        serde_json::Value::Object(o) => {
             let mut map = std::collections::HashMap::new();
             for (k, v) in o {
                 map.insert(k, convert_json_to_value(v)?);
             }
             Ok(Value::Map(map))
        }
    }
}

fn convert_value_to_json(v: &Value) -> serde_json::Value {
    match v {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Int(n) => serde_json::Value::Number((*n).into()),
        Value::Float(f) => serde_json::Number::from_f64(*f).map(serde_json::Value::Number).unwrap_or(serde_json::Value::Null),
        Value::Str(s) => serde_json::Value::String(s.clone()),
        Value::Array(a) => {
            serde_json::Value::Array(a.iter().map(convert_value_to_json).collect())
        },
        Value::Map(m) => {
            let mut map = serde_json::Map::new();
            for (k, v) in m {
                map.insert(k.clone(), convert_value_to_json(v));
            }
            serde_json::Value::Object(map)
        },
        _ => serde_json::Value::String(format!("{}", v)), // Fallback for functions etc
    }
}
