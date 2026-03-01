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

pub fn call(func_name: &str, args: &[Value]) -> Result<Value, String> {
    // Helper to simulate Kinetix Result<T, E> enum
    let ok_res = |v: Value| -> Value {
        let mut m = std::collections::HashMap::new();
        m.insert("ok".to_string(), v);
        Value::Map(m)
    };
    let err_res = |msg: &str| -> Value {
        let mut m = std::collections::HashMap::new();
        m.insert("err".to_string(), Value::Str(msg.to_string()));
        Value::Map(m)
    };

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
            Ok(Value::Int((sys.free_memory() / 1024 / 1024) as i64))
        },
        "memory_total" => {
            let mut sys = SYS.lock().map_err(|_| "Global system context lock failed")?;
            sys.refresh_memory();
            Ok(Value::Int((sys.total_memory() / 1024 / 1024) as i64))
        },
        "os.name" | "os_name" => {
            let name = sysinfo::System::name().unwrap_or("Unknown".into());
            Ok(ok_res(Value::Str(name)))
        },
        "os.arch" => {
            Ok(ok_res(Value::Str(std::env::consts::ARCH.to_string())))
        },
        "os_version" => {
            let ver = sysinfo::System::os_version().unwrap_or("Unknown".into());
            Ok(Value::Str(ver))
        },
        "os.isWindows" | "isWindows" => {
            Ok(Value::Bool(cfg!(windows)))
        },
        "os.isLinux" | "isLinux" => {
            Ok(Value::Bool(cfg!(target_os = "linux")))
        },
        "os.isMac" | "isMac" => {
            Ok(Value::Bool(cfg!(target_os = "macos")))
        },
        "exec" => {
            if let Some(Value::Str(cmd)) = args.first() {
                // Security: Capabilities check should happen at compile-time in sandbox auditor
                let output = std::process::Command::new(if cfg!(windows) { "cmd.exe" } else { "sh" })
                    .arg(if cfg!(windows) { "/c" } else { "-c" })
                    .arg(cmd)
                    .output();
                
                match output {
                    Ok(out) => {
                        let mut res = std::collections::HashMap::new();
                        res.insert("stdout".to_string(), Value::Str(String::from_utf8_lossy(&out.stdout).to_string()));
                        res.insert("stderr".to_string(), Value::Str(String::from_utf8_lossy(&out.stderr).to_string()));
                        res.insert("status".to_string(), Value::Int(out.status.code().unwrap_or(-1) as i64));
                        Ok(ok_res(Value::Map(res)))
                    }
                    Err(e) => Ok(err_res(&format!("exec failed: {}", e)))
                }
            } else {
                Ok(err_res("system.exec requires a string command"))
            }
        },
        "thread.spawn" => {
            Ok(err_res("Not Implemented: thread.spawn requires linear types & ownership handoff"))
        },
        "thread.join" => {
            Ok(err_res("Not Implemented: thread.join"))
        },
        "thread.sleep" => {
            if let Some(n) = args.first() {
                let ms = n.as_int().unwrap_or(0);
                std::thread::sleep(std::time::Duration::from_millis(ms as u64));
                Ok(ok_res(Value::Null))
            } else {
                Ok(err_res("system.thread.sleep requires milliseconds (int)"))
            }
        },
        "defer" => {
            Ok(err_res("Not Implemented: defer requires compiler scope tracking rules"))
        },
        "hostname" => {
            let host = sysinfo::System::host_name().unwrap_or("Unknown".into());
            Ok(Value::Str(host))
        },
        "user_name" => {
            let user = std::env::var("USERNAME").or(std::env::var("USER")).unwrap_or("Unknown".into());
            Ok(Value::Str(user))
        },
        "uptime" => {
            let uptime = sysinfo::System::uptime();
            Ok(Value::Int(uptime as i64))
        },
        "clipboard_set" | "clipboard_get" => {
            Ok(err_res("Clipboard not implemented (requires dependency)"))
        },
        _ => Err(format!("Unknown System function: {}", func_name))
    }
}
