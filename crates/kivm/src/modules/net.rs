use crate::vm::Value;

use ureq;

pub fn call(func_name: &str, args: &[Value]) -> Result<Value, String> {
    match func_name {
        "get" => {
            if let Some(Value::Str(url)) = args.first() {
                match ureq::get(url).call() {
                    Ok(resp) => {
                         let text = resp.into_string().map_err(|e| e.to_string())?;
                         // Return dictionary { status: 200, body: "..." } ?
                         // Or just body string for simplicity given "ret: Future<Response>" in docs (which was async).
                         // Since we are synchronous blocking here (KiVM is single-threaded mostly), we return body string or simple struct.
                         // Docs say "Future<Response>". We are implementing synchronous version for now as KiVM async isn't fully ready.
                         Ok(Value::Str(text))
                    },
                    Err(e) => Err(format!("Request failed: {}", e))
                }
            } else { Err("Expected URL string".into()) }
        },
        "post" => {
            if let (Some(Value::Str(url)), Some(body_val)) = (args.get(0), args.get(1)) {
                let body_str = format!("{}", body_val); // Simple stringify
                // Attempt JSON if looks like it? Or just text.
                // ureq::post(url).send_string(&body_str)
                 match ureq::post(url).send_string(&body_str) {
                    Ok(resp) => {
                         let text = resp.into_string().map_err(|e| e.to_string())?;
                         Ok(Value::Str(text))
                    },
                    Err(e) => Err(format!("Request failed: {}", e))
                }
            } else { Err("Expected URL and body".into()) }
        },
        "download" => {
             if let (Some(Value::Str(url)), Some(Value::Str(dest))) = (args.get(0), args.get(1)) {
                 match ureq::get(url).call() {
                    Ok(resp) => {
                         let mut reader = resp.into_reader();
                         let mut file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
                         std::io::copy(&mut reader, &mut file).map_err(|e| e.to_string())?;
                         Ok(Value::Null)
                    },
                    Err(e) => Err(format!("Download failed: {}", e))
                }
             } else { Err("Expected URL and destination path".into()) }
        },
        _ => Err(format!("Unknown Net function: {}", func_name))
    }
}
