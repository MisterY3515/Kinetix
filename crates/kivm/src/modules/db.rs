use crate::vm::Value;
use std::sync::Mutex;
use std::collections::HashMap;
use lazy_static::lazy_static;
use rusqlite::Connection;
use std::sync::atomic::{AtomicUsize, Ordering};

lazy_static! {
    static ref CONNECTIONS: Mutex<HashMap<usize, Connection>> = Mutex::new(HashMap::new());
    static ref NEXT_ID: AtomicUsize = AtomicUsize::new(1);
}

pub fn call(name: &str, args: &[Value]) -> Result<Value, String> {
    if name == "connect" {
        return connect(args);
    }

    // Handle object methods: "db_conn:ID.method"
    if name.starts_with("db_conn:") {
        let parts: Vec<&str> = name.split('.').collect();
        if parts.len() != 2 { return Err("Invalid DB method call".into()); }
        
        let handle_part = parts[0]; // "db_conn:1"
        let method = parts[1];      // "query"
        
        let id_str = handle_part.strip_prefix("db_conn:").ok_or("Invalid ID format")?;
        let id = id_str.parse::<usize>().map_err(|_| "Invalid ID")?;

        match method {
            "query" => return query(id, args),
            "execute" => return execute(id, args),
            "close" => return close(id),
            _ => return Err(format!("Unknown DB method: {}", method)),
        }
    }

    Err(format!("Unknown DB function: {}", name))
}

fn connect(args: &[Value]) -> Result<Value, String> {
    let uri = args.first().and_then(|v| match v { Value::Str(s) => Some(s), _ => None }).ok_or("Expected URI string")?;
    
    // Support "sqlite://path"
    let path = if uri.starts_with("sqlite://") {
        uri.strip_prefix("sqlite://").unwrap()
    } else {
        uri
    };

    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    CONNECTIONS.lock().unwrap().insert(id, conn);
    
    // Return a NativeModule acting as the connection object
    Ok(Value::NativeModule(format!("db_conn:{}", id)))
}

fn query(id: usize, args: &[Value]) -> Result<Value, String> {
    let sql = args.first().and_then(|v| match v { Value::Str(s) => Some(s), _ => None }).ok_or("Expected SQL string")?;
    let params_val = args.get(1); // Optional params array

    let connections = CONNECTIONS.lock().unwrap();
    let conn = connections.get(&id).ok_or("Connection closed or invalid")?;

    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;

    // Convert abstract Values to Trait Objects for rusqlite
    let mut sql_params = Vec::new();
    if let Some(Value::Array(arr)) = params_val {
        for v in arr {
            sql_params.push(value_to_sql(v));
        }
    }
    
    // Create a slice of references for query
    let params_refs: Vec<&dyn rusqlite::types::ToSql> = sql_params.iter().map(|b| &**b).collect();

    let col_names: Vec<String> = stmt.column_names().into_iter().map(|s| s.to_string()).collect();

    let mut rows = stmt.query(&*params_refs).map_err(|e| e.to_string())?;

    let mut rows_list = Vec::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let mut map = HashMap::new();
        // Since we collected col_names, we can use them.
        // We know the index matches because column_names order is consistent with row indices.
        for (i, name) in col_names.iter().enumerate() {
            // Need to get value by index
            let val = row_get_value(row, i)?;
            map.insert(name.clone(), val);
        }
        rows_list.push(Value::Map(map));
    }

    Ok(Value::Array(rows_list))
}

fn execute(id: usize, args: &[Value]) -> Result<Value, String> {
    let sql = args.first().and_then(|v| match v { Value::Str(s) => Some(s), _ => None }).ok_or("Expected SQL string")?;
    let params_val = args.get(1);

    let connections = CONNECTIONS.lock().unwrap();
    let conn = connections.get(&id).ok_or("Connection closed or invalid")?;

    let mut sql_params = Vec::new();
    if let Some(Value::Array(arr)) = params_val {
        for v in arr {
            sql_params.push(value_to_sql(v));
        }
    }
    let params_refs: Vec<&dyn rusqlite::types::ToSql> = sql_params.iter().map(|b| &**b).collect();

    let affected = conn.execute(sql, &*params_refs).map_err(|e| e.to_string())?;
    Ok(Value::Int(affected as i64))
}

fn close(id: usize) -> Result<Value, String> {
    CONNECTIONS.lock().unwrap().remove(&id);
    Ok(Value::Null)
}

// Helpers
fn value_to_sql(v: &Value) -> Box<dyn rusqlite::types::ToSql> {
    match v {
        Value::Int(n) => Box::new(*n),
        Value::Float(f) => Box::new(*f),
        Value::Str(s) => Box::new(s.clone()),
        Value::Bool(b) => Box::new(*b),
        Value::Null => Box::new(rusqlite::types::Null),
        _ => Box::new(v.to_string()), // Fallback
    }
}

fn row_get_value(row: &rusqlite::Row, idx: usize) -> Result<Value, String> {
    // We need to know the type or try generic get
    // rusqlite `get_ref` returns `ValueRef`.
    let val_ref = row.get_ref(idx).map_err(|e| e.to_string())?;
    match val_ref {
        rusqlite::types::ValueRef::Null => Ok(Value::Null),
        rusqlite::types::ValueRef::Integer(i) => Ok(Value::Int(i)),
        rusqlite::types::ValueRef::Real(f) => Ok(Value::Float(f)),
        rusqlite::types::ValueRef::Text(b) => Ok(Value::Str(String::from_utf8_lossy(b).to_string())),
        rusqlite::types::ValueRef::Blob(_) => Ok(Value::Str("<blob>".into())), // Binaries as string placeholder or array?
    }
}


