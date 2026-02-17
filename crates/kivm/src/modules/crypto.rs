use crate::vm::Value;
use sha2::{Sha256, Digest};
use hmac::{Hmac, Mac};
use uuid::Uuid;

pub fn call(func_name: &str, args: &[Value]) -> Result<Value, String> {
    match func_name {
        "hash" => {
            if let Some(Value::Str(data)) = args.first() {
                let mut hasher = Sha256::new();
                hasher.update(data.as_bytes());
                let result = hasher.finalize();
                Ok(Value::Str(hex::encode(result)))
            } else { Err("Expected string data".into()) }
        },
        "hmac" => {
            if let (Some(Value::Str(key)), Some(Value::Str(data))) = (args.get(0), args.get(1)) {
                 type HmacSha256 = Hmac<Sha256>;
                 let mut mac = HmacSha256::new_from_slice(key.as_bytes())
                    .map_err(|e| e.to_string())?;
                 mac.update(data.as_bytes());
                 let result = mac.finalize();
                 Ok(Value::Str(hex::encode(result.into_bytes())))
            } else { Err("Expected key and data strings".into()) }
        },
        "uuid" => {
            let id = Uuid::new_v4();
            Ok(Value::Str(id.to_string()))
        },
        "random_bytes" => {
             if let Some(Value::Int(size)) = args.first() {
                 let size = *size as usize;
                 let mut bytes = vec![0u8; size];
                 // sysinfo or rand? rand is better.
                 // We have rand dependency.
                 // rand::thread_rng().fill_bytes(&mut bytes); // if rand available
                 let mut rng = rand::thread_rng();
                 use rand::RngCore;
                 rng.fill_bytes(&mut bytes);
                 Ok(Value::Str(hex::encode(bytes))) // Return hex string for buffer? Or array of ints?
                 // Docs say "Buffer", but Value doesn't have Buffer. 
                 // We'll return hex string for now or Array of ints.
                 // let arr = bytes.iter().map(|b| Value::Int(*b as i64)).collect();
                 // Ok(Value::Array(arr))
             } else { Err("Expected size int".into()) }
        },
        _ => Err(format!("Unknown Crypto function: {}", func_name))
    }
}
