/// Built-in functions for KixVM.

use crate::vm::Value;

#[path = "modules/mod.rs"]
pub mod modules;

pub const BUILTIN_NAMES: &[&str] = &[
    // Core
    "print", "println", "input", "len", "typeof", "assert",
    "str", "int", "float", "bool", "byte", "char", "stop", "exit", "copy",
    
    // String Globals
    "to_upper", "to_lower", "trim", "split", "replace", "contains", 
    "starts_with", "ends_with", "pad_left", "pad_right", "join",

    // List Globals
    "push", "pop", "remove_at", "insert", "reverse", "sort", 
    "min", "max", "any", "all",

    // Iteration
    "range", "enumerate", "zip", "map", "filter", "reduce",
    
    // Math Globals (wrapper/alias if needed, usually accessed via Math.)
    "Math.abs", "Math.ceil", "Math.floor", "Math.round", "Math.pow", "Math.sqrt",
    "Math.sin", "Math.cos", "Math.tan", "Math.asin", "Math.acos", "Math.atan2",
    "Math.deg", "Math.rad", "Math.cbrt", "Math.exp", "Math.log", "Math.log10",
    "Math.clamp", "Math.lerp", "Math.min", "Math.max", "Math.random", "Math.random_range", 
    "math.distance_sq", "math.dot", "math.cross", "math.normalize",
    "System.time", "time.now", "time.ticks", "time.sleep",
    "system.os.isWindows", "system.os.isLinux", "system.os.isMac",
    "system.os.name", "system.os.arch", "system.exec",
    "system.thread.spawn", "system.thread.join", "system.thread.sleep", "system.defer",
    "env.get", "env.set", "env.args",
];

use crate::vm::VM;

pub fn call_builtin(name: &str, args: &[Value], vm: &mut VM) -> Result<Value, String> {
    match name {
        // --- Core ---
        "print" | "println" => {
            let text: Vec<String> = args.iter().map(|a| format!("{}", a)).collect();
            let line = text.join(" ");
            println!("{}", line);
            vm.output.push(line);
            Ok(Value::Null)
        }
        "input" => {
            if let Some(Value::Str(prompt)) = args.first() { print!("{}", prompt); }
            let mut buf = String::new();
            std::io::stdin().read_line(&mut buf).map_err(|e| e.to_string())?;
            Ok(Value::Str(buf.trim().to_string()))
        }
        "len" => match args.first() { // Global len
            Some(Value::Str(s)) => Ok(Value::Int(s.len() as i64)),
            Some(Value::Array(a)) => Ok(Value::Int(a.len() as i64)),
            Some(Value::Map(m)) => Ok(Value::Int(m.len() as i64)),
            _ => Ok(Value::Int(0)),
        },
        "typeof" => {
            let t = match args.first() {
                Some(Value::Int(_)) => "int",
                Some(Value::Float(_)) => "float",
                Some(Value::Str(_)) => "string",
                Some(Value::Bool(_)) => "bool",
                Some(Value::Null) => "null",
                Some(Value::Array(_)) => "array",
                Some(Value::Function(_)) => "function",
                Some(Value::NativeFn(_)) => "native_function",
                Some(Value::NativeModule(_)) => "module",
                Some(Value::BoundMethod(_, _)) => "bound_method",
                Some(Value::Map(_)) => "map",
                None => "void",
            };
            Ok(Value::Str(t.to_string()))
        }
        "assert" => {
            let cond = args.first().map(|v| v.is_truthy()).unwrap_or(false);
            if !cond {
                let msg = args.get(1).map(|v| format!("{}", v)).unwrap_or_else(|| "Assertion failed".into());
                return Err(format!("Assertion failed: {}", msg));
            }
            Ok(Value::Null)
        }
        "stop" | "exit" | "System.exit" => {
            let code = args.first().and_then(|v| match v {
                Value::Int(n) => Some(*n as i32),
                _ => None,
            }).unwrap_or(0);
            std::process::exit(code);
        }

        "copy" => Ok(args.first().cloned().unwrap_or(Value::Null)),
        
        // --- Global String Wrappers ---
        "to_upper" => call_builtin("str.upper", args, vm),
        "to_lower" => call_builtin("str.lower", args, vm),
        "trim" => call_builtin("str.trim", args, vm),
        "split" => call_builtin("str.split", args, vm),
        "replace" => call_builtin("str.replace", args, vm),
        "contains" => {
            match args.first() {
                Some(Value::Str(_)) => call_builtin("str.contains", args, vm),
                Some(Value::Array(_)) => call_builtin("array.contains", args, vm),
                _ => Ok(Value::Bool(false)),
            }
        },

        // --- Global List Wrappers ---
        "push" => call_builtin("array.push", args, vm),
        "pop" => call_builtin("array.pop", args, vm),
        "remove_at" => {
             if let (Some(Value::Array(arr)), Some(Value::Int(idx))) = (args.get(0), args.get(1)) {
                 let mut new_arr = arr.clone();
                 if *idx >= 0 && (*idx as usize) < new_arr.len() {
                     new_arr.remove(*idx as usize);
                 }
                 Ok(Value::Array(new_arr))
             } else { Ok(Value::Null) }
        },
        "insert" => {
             if let (Some(Value::Array(arr)), Some(Value::Int(idx)), Some(val)) = (args.get(0), args.get(1), args.get(2)) {
                 let mut new_arr = arr.clone();
                 let idx = (*idx as usize).min(new_arr.len());
                 new_arr.insert(idx, val.clone());
                 Ok(Value::Array(new_arr))
             } else { Ok(Value::Null) }
        },
        "reverse" => call_builtin("array.reverse", args, vm),
        "sort" => call_builtin("array.sort", args, vm),
        
        "any" => {
            if let (Some(Value::Array(_arr)), Some(Value::Function(_))) = (args.get(0), args.get(1)) {
                // Cannot execute callback here easily without &mut VM
                Err("Feature 'any' with callback not yet supported in builtins".into())
            } else { Err("Invalid args for any".into()) } 
        },
        "all" => {
            if let (Some(Value::Array(_arr)), Some(Value::Function(_))) = (args.get(0), args.get(1)) {
                // Cannot execute callback here easily without &mut VM
                Err("Feature 'all' with callback not yet supported in builtins".into())
            } else { Err("Invalid args for all".into()) }
        },

        "min" => {
            if args.len() == 1 {
                if let Some(Value::Array(arr)) = args.first() {
                    // Find min in array
                    let min_val = arr.iter().min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    Ok(min_val.cloned().unwrap_or(Value::Null))
                } else { Ok(args[0].clone()) }
            } else {
                call_builtin("Math.min", args, vm)
            }
        },
        "max" => {
            if args.len() == 1 {
                 if let Some(Value::Array(arr)) = args.first() {
                    // Find max in array
                    let max_val = arr.iter().max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    Ok(max_val.cloned().unwrap_or(Value::Null))
                } else { Ok(args[0].clone()) }
            } else {
                call_builtin("Math.max", args, vm)
            }
        },

        "starts_with" => {
             if let (Some(Value::Str(s)), Some(Value::Str(prefix))) = (args.get(0), args.get(1)) {
                 Ok(Value::Bool(s.starts_with(prefix)))
             } else { Ok(Value::Bool(false)) }
        },
        "ends_with" => {
             if let (Some(Value::Str(s)), Some(Value::Str(suffix))) = (args.get(0), args.get(1)) {
                 Ok(Value::Bool(s.ends_with(suffix)))
             } else { Ok(Value::Bool(false)) }
        },
        "pad_left" => {
             if let (Some(Value::Str(s)), Some(Value::Int(len))) = (args.get(0), args.get(1)) {
                 let pad_char = args.get(2).and_then(|v| if let Value::Str(c) = v { c.chars().next() } else { None }).unwrap_or(' ');
                 let width = *len as usize;
                 if s.len() >= width { Ok(Value::Str(s.clone())) } else {
                     let pad = std::iter::repeat(pad_char).take(width - s.len()).collect::<String>();
                     Ok(Value::Str(format!("{}{}", pad, s)))
                 }
             } else { Ok(Value::Null) }
        },
        "pad_right" => {
             if let (Some(Value::Str(s)), Some(Value::Int(len))) = (args.get(0), args.get(1)) {
                 let pad_char = args.get(2).and_then(|v| if let Value::Str(c) = v { c.chars().next() } else { None }).unwrap_or(' ');
                 let width = *len as usize;
                 if s.len() >= width { Ok(Value::Str(s.clone())) } else {
                     let pad = std::iter::repeat(pad_char).take(width - s.len()).collect::<String>();
                     Ok(Value::Str(format!("{}{}", s, pad)))
                 }
             } else { Ok(Value::Null) }
        },
        "join" => {
             if let (Some(Value::Array(list)), Some(Value::Str(sep))) = (args.get(0), args.get(1)) {
                 let joined = list.iter().map(|v| format!("{}", v)).collect::<Vec<_>>().join(sep);
                 Ok(Value::Str(joined))
             } else { Ok(Value::Null) }
        },

        // --- Math Module ---
        "Math.sin" => Ok(Value::Float(args.first().and_then(|v| v.as_float().ok()).unwrap_or(0.0).sin())),

        // --- Math Extras ---
        "Math.asin" => Ok(Value::Float(args.first().and_then(|v| v.as_float().ok()).unwrap_or(0.0).asin())),
        "Math.acos" => Ok(Value::Float(args.first().and_then(|v| v.as_float().ok()).unwrap_or(0.0).acos())),
        "Math.atan2" => {
            let y = args.get(0).and_then(|v| v.as_float().ok()).unwrap_or(0.0);
            let x = args.get(1).and_then(|v| v.as_float().ok()).unwrap_or(0.0);
            Ok(Value::Float(y.atan2(x)))
        },
        "Math.deg" => Ok(Value::Float(args.first().and_then(|v| v.as_float().ok()).unwrap_or(0.0).to_degrees())),
        "Math.rad" => Ok(Value::Float(args.first().and_then(|v| v.as_float().ok()).unwrap_or(0.0).to_radians())),
        "Math.cbrt" => Ok(Value::Float(args.first().and_then(|v| v.as_float().ok()).unwrap_or(0.0).cbrt())),
        "Math.exp" => Ok(Value::Float(args.first().and_then(|v| v.as_float().ok()).unwrap_or(0.0).exp())),
        "Math.log" => Ok(Value::Float(args.first().and_then(|v| v.as_float().ok()).unwrap_or(0.0).ln())),
        "Math.log10" => Ok(Value::Float(args.first().and_then(|v| v.as_float().ok()).unwrap_or(0.0).log10())),
        "Math.clamp" => {
            let val = args.get(0);
            let min = args.get(1);
            let max = args.get(2);
            match (val, min, max) {
                (Some(Value::Int(v)), Some(Value::Int(mn)), Some(Value::Int(mx))) => {
                    Ok(Value::Int(*v.max(mn).min(mx)))
                },
                (Some(v), Some(mn), Some(mx)) => {
                     let vf = v.as_float().unwrap_or(0.0);
                     let mnf = mn.as_float().unwrap_or(0.0);
                     let mxf = mx.as_float().unwrap_or(1.0);
                     Ok(Value::Float(vf.max(mnf).min(mxf)))
                },
                _ => Ok(Value::Null)
            }
        },
        "Math.lerp" => {
            let a = args.get(0).and_then(|v| v.as_float().ok()).unwrap_or(0.0);
            let b = args.get(1).and_then(|v| v.as_float().ok()).unwrap_or(0.0);
            let t = args.get(2).and_then(|v| v.as_float().ok()).unwrap_or(0.0);
            Ok(Value::Float(a + (b - a) * t))
        },
        
        // --- Iterators (Returning Lists currently) ---
        "range" => {
            let start = args.get(0).and_then(|v| v.as_int().ok()).unwrap_or(0);
            let end = args.get(1).and_then(|v| v.as_int().ok()).unwrap_or(0);
            let step = args.get(2).and_then(|v| v.as_int().ok()).unwrap_or(1);
            let mut res = Vec::new();
            let mut i = start;
            if step > 0 {
                while i < end { res.push(Value::Int(i)); i += step; }
            } else if step < 0 {
                while i > end { res.push(Value::Int(i)); i += step; }
            }
            Ok(Value::Array(res))
        },
        "enumerate" => {
             if let Some(Value::Array(arr)) = args.first() {
                 let res = arr.iter().enumerate().map(|(i, v)| {
                     Value::Array(vec![Value::Int(i as i64), v.clone()])
                 }).collect();
                 Ok(Value::Array(res))
             } else { Ok(Value::Null) }
        },
        "zip" => {
             if let (Some(Value::Array(a)), Some(Value::Array(b))) = (args.get(0), args.get(1)) {
                 let len = a.len().min(b.len());
                 let mut res = Vec::with_capacity(len);
                 for i in 0..len {
                     res.push(Value::Array(vec![a[i].clone(), b[i].clone()]));
                 }
                 Ok(Value::Array(res))
             } else { Ok(Value::Null) }
        },
        "map" | "filter" | "reduce" => Err("Functional iterators require callbacks (not implemented yet)".into()),

        // --- Conversions ---
        "byte" => {
             let n = args.first().and_then(|v| v.as_int().ok()).unwrap_or(0);
             Ok(Value::Int((n & 0xFF) as i64))
        },
        "char" => {
             let n = args.first().and_then(|v| v.as_int().ok()).unwrap_or(0);
             if let Some(c) = std::char::from_u32(n as u32) {
                 Ok(Value::Str(c.to_string()))
             } else {
                 Err(format!("Invalid char code: {}", n))
             }
        },

        "Math.cos" => Ok(Value::Float(args.first().and_then(|v| v.as_float().ok()).unwrap_or(0.0).cos())),
        "Math.tan" => Ok(Value::Float(args.first().and_then(|v| v.as_float().ok()).unwrap_or(0.0).tan())),
        "Math.sqrt" => Ok(Value::Float(args.first().and_then(|v| v.as_float().ok()).unwrap_or(0.0).sqrt())),
        "Math.abs" => match args.first() {
             Some(Value::Int(i)) => Ok(Value::Int(i.abs())),
             Some(Value::Float(f)) => Ok(Value::Float(f.abs())),
             _ => Ok(Value::Int(0)),
        },
        "Math.floor" => Ok(Value::Float(args.first().and_then(|v| v.as_float().ok()).unwrap_or(0.0).floor())),
        "Math.ceil" => Ok(Value::Float(args.first().and_then(|v| v.as_float().ok()).unwrap_or(0.0).ceil())),
        "Math.round" => Ok(Value::Float(args.first().and_then(|v| v.as_float().ok()).unwrap_or(0.0).round())),
        "Math.pow" => {
            let base = args.get(0).and_then(|v| v.as_float().ok()).unwrap_or(0.0);
            let exp = args.get(1).and_then(|v| v.as_float().ok()).unwrap_or(1.0);
            Ok(Value::Float(base.powf(exp)))
        }
        "Math.min" => {
             match (args.get(0), args.get(1)) {
                 (Some(Value::Int(a)), Some(Value::Int(b))) => Ok(Value::Int(*a.min(b))),
                 (Some(val_a), Some(val_b)) => {
                     let a = val_a.as_float().unwrap_or(0.0);
                     let b = val_b.as_float().unwrap_or(0.0);
                     Ok(Value::Float(a.min(b)))
                 }
                 _ => Ok(Value::Null)
             }
        },
        "Math.max" => {
             match (args.get(0), args.get(1)) {
                 (Some(Value::Int(a)), Some(Value::Int(b))) => Ok(Value::Int(*a.max(b))),
                 (Some(val_a), Some(val_b)) => {
                     let a = val_a.as_float().unwrap_or(0.0);
                     let b = val_b.as_float().unwrap_or(0.0);
                     Ok(Value::Float(a.max(b)))
                 }
                 _ => Ok(Value::Null)
             }
        }
        "Math.random" => Ok(Value::Float(rand::random())), 
        "Math.random_range" => {
            let min = args.get(0).and_then(|v| v.as_float().ok()).unwrap_or(0.0);
            let max = args.get(1).and_then(|v| v.as_float().ok()).unwrap_or(1.0);
            // Basic implementation using standard rand if available or pseudo logic
            // Assuming rand crate is available as per line 91
            let r = rand::random::<f64>(); 
            Ok(Value::Float(min + r * (max - min)))
        }
        
        // --- System Module ---
        "System.time" | "time.now" => {
            modules::system::call("time", args) // Fixed path and semicolon
        },
        s if s.starts_with("System.") => {
            let func = s.strip_prefix("System.").unwrap();
            modules::system::call(func, args)
        },
        s if s.starts_with("sys.") => {
             let func = s.strip_prefix("sys.").unwrap();
             modules::system::call(func, args)
        },

        // --- Net Module ---
        s if s.starts_with("Net.") => {
            let func = s.strip_prefix("Net.").unwrap();
            modules::net::call(func, args)
        },
         "net.get" => modules::net::call("get", args),
         "net.post" => modules::net::call("post", args),
         "net.download" => modules::net::call("download", args),

        // --- Crypto Module ---
        s if s.starts_with("Crypto.") => {
             let func = s.strip_prefix("Crypto.").unwrap();
             modules::crypto::call(func, args)
        },
        "crypto.hash" => modules::crypto::call("hash", args),
        "crypto.hmac" => modules::crypto::call("hmac", args),
        "crypto.uuid" => modules::crypto::call("uuid", args),
        "crypto.random_bytes" => modules::crypto::call("random_bytes", args),

        // --- Audio Module ---
        s if s.starts_with("Audio.") => {
             let func = s.strip_prefix("Audio.").unwrap();
             modules::audio::call(func, args)
        },
        "audio.play_oneshot" => modules::audio::call("play_oneshot", args),
        "audio.play_stream" => modules::audio::call("play_stream", args),

        // --- Data Module ---
        s if s.starts_with("data.") => {
             let func = s.strip_prefix("data.").unwrap();
             modules::data::call(func, args)
        },
        // JSON & CSV
        "json.parse" => modules::data::call("json.parse", args),
        // "json.stringify" handled by data module if matching exact name, or:
        s if s.starts_with("json.") => modules::data::call(s, args),
        s if s.starts_with("csv.") => modules::data::call(s, args),

        // --- DB Module ---
        s if s.starts_with("db.") => {
             let func = s.strip_prefix("db.").unwrap();
             modules::db::call(func, args)
        },
        // DB Connection Methods (db_conn:ID.method)
        s if s.starts_with("db_conn:") => {
             modules::db::call(s, args)
        },

        // --- Graph Module ---
        s if s.starts_with("graph.") => {
             let func = s.strip_prefix("graph.").unwrap();
             modules::graph::call(func, args, vm)
        },

        // --- LLM Module ---
        s if s.starts_with("llm.") => {
             let func = s.strip_prefix("llm.").unwrap();
             modules::llm::call(func, args, vm)
        },

        // --- Term Module ---
        s if s.starts_with("term.") => {
             let func = s.strip_prefix("term.").unwrap();
             modules::term::call(func, args)
        },

        // --- Env Module Override/Expansion ---
        "env.cwd" => {
            std::env::current_dir()
                .map(|p| Value::Str(p.to_string_lossy().to_string()))
                .map_err(|e| e.to_string())
        },
        "env.set_cwd" => {
            if let Some(Value::Str(path)) = args.first() {
                std::env::set_current_dir(path)
                    .map(|_| Value::Null)
                    .map_err(|e| e.to_string())
            } else { Err("Expected path string".into()) }
        },
        "env.user" => modules::system::call("user_name", args),
        "env.hostname" => modules::system::call("hostname", args),

        
        // --- Env Module ---
        "env.get" => {
             if let Some(Value::Str(key)) = args.first() {
                 match std::env::var(key) {
                     Ok(val) => Ok(Value::Str(val)),
                     Err(_) => Ok(Value::Null),
                 }
             } else { Ok(Value::Null) }
        },
        "env.set" => {
             if let (Some(Value::Str(key)), Some(Value::Str(val))) = (args.get(0), args.get(1)) {
                 unsafe { std::env::set_var(key, val); }
                 Ok(Value::Null)
             } else { Ok(Value::Null) }
        },
        "env.args" => {
             let args: Vec<Value> = std::env::args().map(Value::Str).collect();
             Ok(Value::Array(args))
        },

        // --- Vector Math (Arrays) ---
        "math.vector2" => {
             let x = args.get(0).unwrap_or(&Value::Float(0.0)).clone();
             let y = args.get(1).unwrap_or(&Value::Float(0.0)).clone();
             Ok(Value::Array(vec![x, y]))
        },
        "math.vector3" => {
             let x = args.get(0).unwrap_or(&Value::Float(0.0)).clone();
             let y = args.get(1).unwrap_or(&Value::Float(0.0)).clone();
             let z = args.get(2).unwrap_or(&Value::Float(0.0)).clone();
             Ok(Value::Array(vec![x, y, z]))
        },
        "math.dot" => {
             if let (Some(Value::Array(a)), Some(Value::Array(b))) = (args.get(0), args.get(1)) {
                 let mut sum = 0.0;
                 for (v1, v2) in a.iter().zip(b.iter()) {
                     sum += v1.as_float().unwrap_or(0.0) * v2.as_float().unwrap_or(0.0);
                 }
                 Ok(Value::Float(sum))
             } else { Ok(Value::Float(0.0)) }
        },
        "math.cross" => {
             if let (Some(Value::Array(a)), Some(Value::Array(b))) = (args.get(0), args.get(1)) {
                 if a.len() >= 3 && b.len() >= 3 {
                     let ax = a[0].as_float().unwrap_or(0.0); let ay = a[1].as_float().unwrap_or(0.0); let az = a[2].as_float().unwrap_or(0.0);
                     let bx = b[0].as_float().unwrap_or(0.0); let by = b[1].as_float().unwrap_or(0.0); let bz = b[2].as_float().unwrap_or(0.0);
                     Ok(Value::Array(vec![
                         Value::Float(ay * bz - az * by),
                         Value::Float(az * bx - ax * bz),
                         Value::Float(ax * by - ay * bx)
                     ]))
                 } else { Ok(Value::Null) }
             } else { Ok(Value::Null) }
        },
        "math.length_sq" => {
             if let Some(Value::Array(a)) = args.first() {
                 let sum: f64 = a.iter().map(|v| { let f = v.as_float().unwrap_or(0.0); f*f }).sum();
                 Ok(Value::Float(sum))
             } else { Ok(Value::Float(0.0)) }
        },
        "math.length" | "math.distance" => { // Distance handled if 2 args? No, distance takes 2 args. Split logic.
             // This branch only for 1 arg calls to length? No, regex logic for builtin names is exact match.
             // I need separate cases.
             if name == "math.distance" {
                 if let (Some(Value::Array(a)), Some(Value::Array(b))) = (args.get(0), args.get(1)) {
                     let mut sum = 0.0;
                     for (v1, v2) in a.iter().zip(b.iter()) {
                         let diff = v1.as_float().unwrap_or(0.0) - v2.as_float().unwrap_or(0.0);
                         sum += diff * diff;
                     }
                     Ok(Value::Float(sum.sqrt()))
                 } else { Ok(Value::Float(0.0)) }
             } else {
                 // math.length
                 if let Some(Value::Array(a)) = args.first() {
                     let sum: f64 = a.iter().map(|v| { let f = v.as_float().unwrap_or(0.0); f*f }).sum();
                     Ok(Value::Float(sum.sqrt()))
                 } else { Ok(Value::Float(0.0)) }
             }
        },
        "math.distance_sq" => {
             if let (Some(Value::Array(a)), Some(Value::Array(b))) = (args.get(0), args.get(1)) {
                 let mut sum = 0.0;
                 for (v1, v2) in a.iter().zip(b.iter()) {
                     let diff = v1.as_float().unwrap_or(0.0) - v2.as_float().unwrap_or(0.0);
                     sum += diff * diff;
                 }
                 Ok(Value::Float(sum))
             } else { Ok(Value::Float(0.0)) }
        },
        "math.normalize" => {
             if let Some(Value::Array(a)) = args.first() {
                 let sum: f64 = a.iter().map(|v| { let f = v.as_float().unwrap_or(0.0); f*f }).sum();
                 let len = sum.sqrt();
                 if len == 0.0 { Ok(Value::Array(a.clone())) } else {
                     let res = a.iter().map(|v| Value::Float(v.as_float().unwrap_or(0.0) / len)).collect();
                     Ok(Value::Array(res))
                 }
             } else { Ok(Value::Null) }
        },

        // --- String Methods ---
        "str.len" => Ok(Value::Int(args.first().and_then(|v| if let Value::Str(s) = v { Some(s.len() as i64) } else { None }).unwrap_or(0))),
        "str.upper" => {
             if let Some(Value::Str(s)) = args.first() {
                 Ok(Value::Str(s.to_uppercase()))
             } else { Ok(Value::Null) }
        },
        "str.lower" => {
             if let Some(Value::Str(s)) = args.first() {
                 Ok(Value::Str(s.to_lowercase()))
             } else { Ok(Value::Null) }
        },
        "str.trim" => {
             if let Some(Value::Str(s)) = args.first() {
                 Ok(Value::Str(s.trim().to_string()))
             } else { Ok(Value::Null) }
        },
        "str.contains" => {
             if let (Some(Value::Str(s)), Some(Value::Str(sub))) = (args.get(0), args.get(1)) {
                 Ok(Value::Bool(s.contains(sub)))
             } else { Ok(Value::Bool(false)) }
        },
        "str.replace" => {
             if let (Some(Value::Str(s)), Some(Value::Str(old)), Some(Value::Str(new))) = (args.get(0), args.get(1), args.get(2)) {
                 Ok(Value::Str(s.replace(old, new)))
             } else { Ok(Value::Null) }
        },
        "str.split" => {
             if let (Some(Value::Str(s)), Some(Value::Str(delim))) = (args.get(0), args.get(1)) {
                 let parts = s.split(delim).map(|p| Value::Str(p.to_string())).collect();
                 Ok(Value::Array(parts))
             } else { Ok(Value::Null) }
        },
        
        // --- Array Methods ---
        "array.len" => Ok(Value::Int(args.first().and_then(|v| if let Value::Array(a) = v { Some(a.len() as i64) } else { None }).unwrap_or(0))),
        "array.push" => {
            // Note: This requires mutable access to VM memory/registers which call_builtin doesn't strictly have access to via 'args' slice references alone if we want to modify the original array in place. 
            // However, KiVM passes arrays by reference (sort of, via Clone currently in VM loop... wait).
            // VM::step implementation of Call passes `args` as CLONED values currently.
            // "let func_val = frame.reg(instr.a).clone();"
            // "for i in 0..arg_count { args.push(frame.reg(instr.a + 1 + i as u8).clone()); }"
            // This means `array.push` won't work in-place with current VM architecture unless we change how arrays are passed/stored (Heap/Rc).
            // For now, we returns a NEW array with the item pushed (functional style) or we accept that it's limited.
            // Requirement says "Expanding Library". Let's assume functional for now or just log warning.
            // Actually, let's implement it returning the new array.
            if let Some(Value::Array(arr)) = args.first() {
                 let mut new_arr = arr.clone();
                 if let Some(val) = args.get(1) {
                     new_arr.push(val.clone());
                 }
                 Ok(Value::Array(new_arr))
            } else {
                 Ok(Value::Null)
            }
        },
        "array.pop" => {
            if let Some(Value::Array(arr)) = args.first() {
                 let mut new_arr = arr.clone();
                 new_arr.pop();
                 Ok(Value::Array(new_arr))
            } else {
                 Ok(Value::Null)
            }
        },
        "array.contains" => {
             if let Some(Value::Array(arr)) = args.first() {
                 let target = args.get(1).unwrap_or(&Value::Null);
                 // Need generic logic to compare Values. Assuming simple equality for now.
                 // Value likely derives PartialEq
                 let found = arr.iter().any(|v| v == target);
                 Ok(Value::Bool(found))
             } else {
                 Ok(Value::Bool(false))
             }
        },
        "array.reverse" => {
             if let Some(Value::Array(arr)) = args.first() {
                 let mut new_arr = arr.clone();
                 new_arr.reverse();
                 Ok(Value::Array(new_arr))
             } else { Ok(Value::Null) }
        },
        "array.sort" => {
             if let Some(Value::Array(arr)) = args.first() {
                 let mut new_arr = arr.clone();
                 // Naive sort: convert to string/int comparison or try partial_cmp.
                 // Assuming partial_cmp exists for Value or we implement a lambda.
                 // For now, let's sort assuming homogenous types if possible, or string fallback.
                 // This is tricky without `Ord`.
                 // Let's assume we can't easily sort mixed types and just do nothing or error?
                 // Or basic string sort.
                 // Let's try to unwrap partial_cmp for floats/ints.
                 new_arr.sort_by(|a, b| {
                     a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
                 });
                 Ok(Value::Array(new_arr))
             } else { Ok(Value::Null) }
        },

        // --- Type Conversions ---
        "str" => Ok(Value::Str(format!("{}", args.first().cloned().unwrap_or(Value::Null)))),
        "int" => match args.first() {
            Some(Value::Int(n)) => Ok(Value::Int(*n)),
            Some(Value::Float(f)) => Ok(Value::Int(*f as i64)),
            Some(Value::Str(s)) => s.parse::<i64>().map(Value::Int).map_err(|_| format!("Cannot convert '{}' to int", s)),
            Some(Value::Bool(b)) => Ok(Value::Int(if *b { 1 } else { 0 })),
            _ => Ok(Value::Int(0)),
        },
        "float" => match args.first() {
            Some(Value::Float(f)) => Ok(Value::Float(*f)),
            Some(Value::Int(n)) => Ok(Value::Float(*n as f64)),
            Some(Value::Str(s)) => s.parse::<f64>().map(Value::Float).map_err(|_| format!("Cannot convert '{}' to float", s)),
            _ => Ok(Value::Float(0.0)),
        },
        "bool" => Ok(Value::Bool(args.first().map(|v| v.is_truthy()).unwrap_or(false))),
        
        // --- OS Detection & System Layer ---
        name if name.starts_with("system.") => modules::system::call(&name[7..], args),

        _ => Err(format!("Unknown built-in: {}", name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kinetix_kicomp::ir::CompiledProgram;

    fn dummy_vm() -> VM {
        VM::new(CompiledProgram::new())
    }

    #[test]
    fn test_typeof() {
        let mut vm = dummy_vm();
        let r = call_builtin("typeof", &[Value::Int(42)], &mut vm).unwrap();
        assert!(matches!(r, Value::Str(s) if s == "int"));
    }

    #[test]
    fn test_len() {
        let mut vm = dummy_vm();
        assert!(matches!(call_builtin("len", &[Value::Str("hi".into())], &mut vm).unwrap(), Value::Int(2)));
    }

    #[test]
    fn test_assert_pass() {
        let mut vm = dummy_vm();
        assert!(call_builtin("assert", &[Value::Bool(true)], &mut vm).is_ok());
    }

    #[test]
    fn test_assert_fail() {
        let mut vm = dummy_vm();
        assert!(call_builtin("assert", &[Value::Bool(false)], &mut vm).is_err());
    }
}
