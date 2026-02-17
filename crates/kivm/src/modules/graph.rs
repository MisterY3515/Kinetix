use crate::vm::{Value, VM};
use minifb::{Window, WindowOptions, Key};
use std::sync::Mutex;
use lazy_static::lazy_static;

struct ThreadSafeWindow(Window);
unsafe impl Send for ThreadSafeWindow {}
unsafe impl Sync for ThreadSafeWindow {} // Mutex requires Send, lazy_static requires Sync for the Mutex wrapper? No, Mutex is Sync if T is Send.

lazy_static! {
    static ref GLOBAL_WINDOW: Mutex<Option<ThreadSafeWindow>> = Mutex::new(None);
    static ref BUFFER: Mutex<Vec<u32>> = Mutex::new(Vec::new());
    static ref WIDTH: Mutex<usize> = Mutex::new(0);
    static ref HEIGHT: Mutex<usize> = Mutex::new(0);
}

pub fn call(name: &str, args: &[Value], vm: &mut VM) -> Result<Value, String> {
    match name {
        "window" => {
            let title = args.get(0).and_then(|v| match v { Value::Str(s) => Some(s), _ => None }).unwrap_or(&"Kinetix Window".to_string()).clone();
            let w = args.get(1).and_then(|v| v.as_int().ok()).unwrap_or(640) as usize;
            let h = args.get(2).and_then(|v| v.as_int().ok()).unwrap_or(480) as usize;
            let callback = args.get(3).cloned(); // Optional callback

            let mut window = Window::new(
                &title,
                w,
                h,
                WindowOptions::default(),
            ).map_err(|e| e.to_string())?;

            // Limit to ~60 fps
            window.limit_update_rate(Some(std::time::Duration::from_micros(16600)));

            // Init Buffer
            {
                let mut buffer = BUFFER.lock().unwrap();
                *buffer = vec![0; w * h];
                let mut gw = WIDTH.lock().unwrap(); *gw = w;
                let mut gh = HEIGHT.lock().unwrap(); *gh = h;
            }

            while window.is_open() && !window.is_key_down(Key::Escape) {
                // Execute Callback
                if let Some(ref cb) = callback {
                     vm.call_value(cb.clone(), vec![], 0).map_err(|e| e.to_string())?;
                     
                     let target_depth = vm.call_stack_len() - 1; 
                     loop {
                         if vm.call_stack_len() <= target_depth { break; }
                         if let crate::vm::StepResult::Halt = vm.step()? {
                             return Ok(Value::Null); // Program halted
                         }
                     }
                }

                // Update Window
                let buffer = BUFFER.lock().unwrap();
                window.update_with_buffer(&buffer, w, h).map_err(|e| e.to_string())?;
            }
            Ok(Value::Null)
        },
        "draw_pixel" => {
             let x = args.get(0).and_then(|v| v.as_int().ok()).unwrap_or(0) as usize;
             let y = args.get(1).and_then(|v| v.as_int().ok()).unwrap_or(0) as usize;
             let color = args.get(2).and_then(|v| v.as_int().ok()).unwrap_or(0xFFFFFF) as u32; // RGB logic needed?
             
             let w = *WIDTH.lock().unwrap();
             if x < w {
                 let mut buf = BUFFER.lock().unwrap();
                 if let Some(pixel) = buf.get_mut(y * w + x) {
                     *pixel = color;
                 }
             }
             Ok(Value::Null)
        },
        "clear" => {
             let color = args.get(0).and_then(|v| v.as_int().ok()).unwrap_or(0) as u32;
             let mut buf = BUFFER.lock().unwrap();
             for p in buf.iter_mut() { *p = color; }
             Ok(Value::Null)
        },
        _ => Err(format!("Unknown graph function: {}", name))
    }
}
