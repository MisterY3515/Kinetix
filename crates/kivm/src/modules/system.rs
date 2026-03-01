use crate::vm::Value;
use std::sync::{Arc, Mutex};
use sysinfo::System;

// Lazy initialization of System info to avoid overhead on every call
// We use a global static Mutex for simplicity in this context
// Note: In a real heavy production VM, we might want this in the VM struct state.
// valid for this architecture:
lazy_static::lazy_static! {
    static ref SYS: Arc<Mutex<System>> = Arc::new(Mutex::new(System::new_all()));
}

// Helper to refresh and get
#[allow(dead_code)]
fn with_system<F, T>(f: F) -> Result<T, String>
where F: FnOnce(&mut System) -> T {
    let mut sys = SYS.lock().map_err(|_| "Global system context lock failed")?;
    // Refreshing everything is expensive. We should refresh specific parts.
    // sys.refresh_all(); 
    Ok(f(&mut sys))
}

pub fn call(func_name: &str, _args: &[Value]) -> Result<Value, String> {
    match func_name {
        "cpu_usage" => {
            let mut sys = SYS.lock().map_err(|_| "Global system context lock failed")?;
            sys.refresh_cpu();
            std::thread::sleep(std::time::Duration::from_millis(100));
            sys.refresh_cpu();
            let usage = sys.global_cpu_info().cpu_usage();
            Ok(Value::Float(usage as f64))
        },
        "memory_free" => {
            let mut sys = SYS.lock().map_err(|_| "Global system context lock failed")?;
            sys.refresh_memory();
            // Convert bytes to MB
            Ok(Value::Int((sys.free_memory() / 1024 / 1024) as i64))
        },
        "memory_total" => {
            let mut sys = SYS.lock().map_err(|_| "Global system context lock failed")?;
            sys.refresh_memory();
            Ok(Value::Int((sys.total_memory() / 1024 / 1024) as i64))
        },
        "os_name" => {
            let name = sysinfo::System::name().unwrap_or("Unknown".into());
            Ok(Value::Str(name))
        },
        "os_version" => {
            let ver = sysinfo::System::os_version().unwrap_or("Unknown".into());
            Ok(Value::Str(ver))
        },
        "isWindows" => {
            Ok(Value::Bool(cfg!(target_os = "windows")))
        },
        "isLinux" => {
            Ok(Value::Bool(cfg!(target_os = "linux")))
        },
        "isMac" => {
            Ok(Value::Bool(cfg!(target_os = "macos")))
        },
        "hostname" => {
            let host = sysinfo::System::host_name().unwrap_or("Unknown".into());
            Ok(Value::Str(host))
        },
        "user_name" => {
            // sysinfo doesn't easily get current user without iterating processes?
            // Actually std::env::var("USERNAME") or "USER" is easier/faster.
            let user = std::env::var("USERNAME").or(std::env::var("USER")).unwrap_or("Unknown".into());
            Ok(Value::Str(user))
        },
        "uptime" => {
            let uptime = sysinfo::System::uptime();
            Ok(Value::Int(uptime as i64))
        },
        "clipboard_set" => {
            // Sysinfo doesn't do clipboard. Need a crate like 'arboard'.
            // For now, return Not Implemented or use a hack.
            // Request said "Implement Not Implemented".
            // I didn't add arboard to Cargo.toml.
            // I'll skip clipboard for now or error.
            Err("Clipboard not implemented (requires dependency)".into())
        },
        "clipboard_get" => {
            Err("Clipboard not implemented (requires dependency)".into())
        },
        _ => Err(format!("Unknown System function: {}", func_name))
    }
}
