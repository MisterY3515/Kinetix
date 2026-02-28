use crate::vm::Value;
use std::fs;
use std::path::Path;

pub fn call(name: &str, args: &[Value]) -> Result<Value, String> {
    match name {
        // --- File IO ---
        "read_text" => {
            let path = args.first().and_then(|v| match v { Value::Str(s) => Some(s), _ => None }).ok_or("Expected path string")?;
            let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
            Ok(Value::Str(content))
        },
        "write_text" => {
            let path = args.get(0).and_then(|v| match v { Value::Str(s) => Some(s), _ => None }).ok_or("Expected path string")?;
            let default_content = String::new();
            let content = args.get(1).and_then(|v| match v { Value::Str(s) => Some(s), _ => None }).unwrap_or(&default_content);
            fs::write(path, content).map_err(|e| e.to_string())?;
            Ok(Value::Null)
        },
        "read_bytes" => {
            let path = args.first().and_then(|v| match v { Value::Str(s) => Some(s), _ => None }).ok_or("Expected path string")?;
            let bytes = fs::read(path).map_err(|e| e.to_string())?;
            // Convert to Array of Ints (slow but compatible)
            let arr = bytes.into_iter().map(|b| Value::Int(b as i64)).collect();
            Ok(Value::Array(arr))
        },
        "exists" => {
             let path = args.first().and_then(|v| match v { Value::Str(s) => Some(s), _ => None }).ok_or("Expected path string")?;
             Ok(Value::Bool(Path::new(path).exists()))
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
             // Just creates an array of zeros
             let size = args.first().and_then(|v| v.as_int().ok()).ok_or("Expected size int")?;
             let arr = vec![Value::Int(0); size as usize];
             Ok(Value::Array(arr))
        },
        "copy" => {
             let src = args.first().and_then(|v| match v { Value::Str(s) => Some(s), _ => None }).ok_or("Expected src path")?;
             let dst = args.get(1).and_then(|v| match v { Value::Str(s) => Some(s), _ => None }).ok_or("Expected dst path")?;
             fs::copy(src, dst).map_err(|e| e.to_string())?;
             Ok(Value::Null)
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
