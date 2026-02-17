use crate::vm::Value;
use std::io::BufReader;
use std::fs::File;
use rodio::{Decoder, OutputStream, Sink};
use std::sync::{Arc, Mutex};

// Store sinks to prevent dropping (stopping audio) immediately
// In a real VM this needs proper resource management
lazy_static::lazy_static! {
    static ref AUDIO_CTX: Arc<Mutex<Vec<Sink>>> = Arc::new(Mutex::new(Vec::new()));
    // OutputStream must be kept alive too? 
    // rodio::OutputStream::try_default() returns (stream, handle).
    // This is tricky in a static context. 
    // For "play_oneshot", we can block or detach?
}

pub fn call(func_name: &str, args: &[Value]) -> Result<Value, String> {
    match func_name {
        "play_oneshot" | "play_stream" => {
            if let Some(Value::Str(path)) = args.first() {
                // This is a blocking implementation for simplicity or naive async.
                // Creating a new stream every time is inefficient.
                // rodio requires keeping the stream alive.
                
                std::thread::spawn({
                    let path = path.clone();
                    move || {
                        let (_stream, stream_handle) = OutputStream::try_default().unwrap();
                        let file = File::open(&path).unwrap();
                        let source = Decoder::new(BufReader::new(file)).unwrap();
                        let sink = Sink::try_new(&stream_handle).unwrap();
                        sink.append(source);
                        sink.sleep_until_end();
                    }
                });
                
                Ok(Value::Null)
            } else { Err("Expected file path".into()) }
        },
        "set_volume" => {
             // Not implemented globally yet
             Ok(Value::Null)
        },
        _ => Err(format!("Unknown Audio function: {}", func_name))
    }
}
