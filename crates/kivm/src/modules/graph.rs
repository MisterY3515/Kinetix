use crate::vm::{Value, VM};
use minifb::{Window, WindowOptions, Key, MouseMode, MouseButton};
use std::sync::Mutex;
use lazy_static::lazy_static;

#[allow(dead_code)]
struct ThreadSafeWindow(Window);
unsafe impl Send for ThreadSafeWindow {}
unsafe impl Sync for ThreadSafeWindow {}

struct UiState {
    mouse_x: f32,
    mouse_y: f32,
    mouse_down: bool,
    keys_pressed: Vec<Key>,
    shift_down: bool,
    #[allow(dead_code)]
    active_id: Option<u64>,
    #[allow(dead_code)]
    hot_id: Option<u64>,
}

lazy_static! {
    static ref GLOBAL_WINDOW: Mutex<Option<ThreadSafeWindow>> = Mutex::new(None);
    static ref BUFFER: Mutex<Vec<u32>> = Mutex::new(Vec::new());
    static ref WIDTH: Mutex<usize> = Mutex::new(0);
    static ref HEIGHT: Mutex<usize> = Mutex::new(0);
    static ref UI_STATE: Mutex<UiState> = Mutex::new(UiState { 
        mouse_x: 0.0, mouse_y: 0.0, mouse_down: false, 
        keys_pressed: Vec::new(), shift_down: false,
        active_id: None, hot_id: None 
    });
}

// Minimal 5x7 Font Data (ASCII 32-127 approx, or just 0-9 A-Z)
// Pack 5 bytes per char. width=5.
#[allow(dead_code)]
const FONT_W: usize = 5;
#[allow(dead_code)]
const FONT_H: usize = 7;
// A small subset for brevity: Space .. Z (ASCII 32..90)
const FONT_DATA: &[u8] = &[
    0,0,0,0,0, // space
    4,4,4,0,4, // !
    10,10,0,0,0, // "
    10,31,10,31,10, // #
    0,0,0,0,0, // $ (skip)
    0,0,0,0,0, // %
    0,0,0,0,0, // &
    0,0,0,0,0, // '
    0,0,0,0,0, // (
    0,0,0,0,0, // )
    0,0,0,0,0, // *
    0,4,14,4,0, // +
    0,0,0,0,0, // ,
    0,0,14,0,0, // -
    0,0,0,0,0, // .
    0,0,0,0,0, // /
    14,17,17,17,14, // 0
    4,12,4,4,14,    // 1
    14,1,14,16,31,  // 2
    31,2,14,1,14,   // 3 (lazy)
    2,6,10,31,2,    // 4
    31,16,14,1,14,  // 5
    6,8,14,17,14,   // 6
    31,1,2,4,4,     // 7
    14,17,14,17,14, // 8
    14,17,14,1,14,  // 9
    0,4,0,4,0,      // :
    0,0,0,0,0,      // ;
    0,0,0,0,0,      // <
    0,0,0,0,0,      // =
    0,0,0,0,0,      // >
    0,0,0,0,0,      // ?
    0,0,0,0,0,      // @
    4,10,17,31,17,  // A
    30,17,30,17,30, // B
    14,17,16,17,14, // C
    28,18,18,18,28, // D
    31,16,30,16,31, // E
    31,16,30,16,16, // F
    14,17,20,17,14, // G (lazy)
    17,17,31,17,17, // H
    14,4,4,4,14,    // I
    1,1,1,17,14,    // J
    17,18,28,18,17, // K
    16,16,16,16,31, // L
    17,27,21,17,17, // M
    17,19,21,25,17, // N
    14,17,17,17,14, // O
    30,17,30,16,16, // P
    14,17,17,21,22, // Q
    30,17,30,20,17, // R
    14,16,14,1,14,  // S
    31,4,4,4,4,     // T
    17,17,17,17,14, // U
    17,17,17,10,4,  // V
    17,21,21,21,10, // W
    17,10,4,10,17,  // X
    17,17,10,4,4,   // Y
    31,2,4,8,31,    // Z
];

fn draw_char(buf: &mut [u32], w: usize, h: usize, c: char, x: usize, y: usize, color: u32) {
    let mut idx = c as usize;
    if idx < 32 || idx > 90 { return; } // Only support subset
    idx -= 32;
    let offset = idx * 5;
    if offset + 5 > FONT_DATA.len() { return; }
    
    for row in 0..5 {
        let byte = FONT_DATA[offset + row];
        for col in 0..5 { // Wait, bitmask? The data above looks like bytes per row? 
            // 5 bytes per char, so 1 byte per row? But wait, font is 5x7? 
            // The data I pasted is 5 bytes per char. 
            // Let's assume 5 bytes = 5 rows? 5x5 font? 
            // The data seems to be 5-byte array where each byte is a ROW (5 bits used).
            // Yes.
            if (byte >> (4 - col)) & 1 == 1 {
                if x + col < w && y + row < h {
                    buf[(y + row) * w + (x + col)] = color;
                }
            }
        }
    }
}

fn draw_text(buf: &mut [u32], w: usize, h: usize, text: &str, x: usize, y: usize, color: u32) {
    let mut cx = x;
    for c in text.chars() {
        draw_char(buf, w, h, c, cx, y, color);
        cx += 6;
    }
}

fn draw_rect(buf: &mut [u32], w: usize, h: usize, rx: usize, ry: usize, rw: usize, rh: usize, color: u32) {
    for y in ry..(ry + rh).min(h) {
        for x in rx..(rx + rw).min(w) {
             if y < h && x < w {
                 buf[y * w + x] = color;
             }
        }
    }
}

#[allow(dead_code)]
fn map_key_to_char(k: Key) -> Option<char> {
    match k {
        Key::Space => Some(' '),
        Key::A => Some('a'), Key::B => Some('b'), Key::C => Some('c'), Key::D => Some('d'),
        Key::E => Some('e'), Key::F => Some('f'), Key::G => Some('g'), Key::H => Some('h'),
        Key::I => Some('i'), Key::J => Some('j'), Key::K => Some('k'), Key::L => Some('l'),
        Key::M => Some('m'), Key::N => Some('n'), Key::O => Some('o'), Key::P => Some('p'),
        Key::Q => Some('q'), Key::R => Some('r'), Key::S => Some('s'), Key::T => Some('t'),
        Key::U => Some('u'), Key::V => Some('v'), Key::W => Some('w'), Key::X => Some('x'),
        Key::Y => Some('y'), Key::Z => Some('z'),
        Key::Key0 | Key::NumPad0 => Some('0'), Key::Key1 | Key::NumPad1 => Some('1'),
        Key::Key2 | Key::NumPad2 => Some('2'), Key::Key3 | Key::NumPad3 => Some('3'),
        Key::Key4 | Key::NumPad4 => Some('4'), Key::Key5 | Key::NumPad5 => Some('5'),
        Key::Key6 | Key::NumPad6 => Some('6'), Key::Key7 | Key::NumPad7 => Some('7'),
        Key::Key8 | Key::NumPad8 => Some('8'), Key::Key9 | Key::NumPad9 => Some('9'),
        _ => None
    }
}

pub fn call(name: &str, args: &[Value], vm: &mut VM) -> Result<Value, String> {
    match name {
        "window" => {
            let title = args.get(0).and_then(|v| match v { Value::Str(s) => Some(s), _ => None }).unwrap_or(&"Kinetix Window".to_string()).clone();
            let w = args.get(1).and_then(|v| v.as_int().ok()).unwrap_or(640) as usize;
            let h = args.get(2).and_then(|v| v.as_int().ok()).unwrap_or(480) as usize;
            let callback = args.get(3).cloned();

            let mut window = Window::new(&title, w, h, WindowOptions::default())
                .map_err(|e| e.to_string())?;
            window.limit_update_rate(Some(std::time::Duration::from_micros(16600)));

            {
                let mut buffer = BUFFER.lock().unwrap();
                *buffer = vec![0; w * h];
                *WIDTH.lock().unwrap() = w;
                *HEIGHT.lock().unwrap() = h;
            }

            {
                let mut ui = UI_STATE.lock().unwrap();
                ui.mouse_x = 0.0; ui.mouse_y = 0.0; ui.mouse_down = false; 
                ui.keys_pressed.clear();
            }

            while window.is_open() && !window.is_key_down(Key::Escape) {
                let (mx, my) = window.get_mouse_pos(MouseMode::Pass).unwrap_or((0.0, 0.0));
                let m_down = window.get_mouse_down(MouseButton::Left);
                let keys = window.get_keys_pressed(minifb::KeyRepeat::Yes);
                let shift = window.is_key_down(Key::LeftShift) || window.is_key_down(Key::RightShift);
                
                {
                    let mut ui = UI_STATE.lock().unwrap();
                    ui.mouse_x = mx;
                    ui.mouse_y = my;
                    ui.mouse_down = m_down;
                    ui.keys_pressed = keys; // Copy keys
                    ui.shift_down = shift;
                }

                if let Some(ref cb) = callback {
                     if let Err(e) = vm.call_value(cb.clone(), vec![], Some(0)) {
                         eprintln!("UI Callback Error: {}", e);
                     }
                     let target_depth = vm.call_stack_len() - 1; 
                     loop {
                         if vm.call_stack_len() <= target_depth { break; }
                         if let crate::vm::StepResult::Halt = vm.step()? {
                             return Ok(Value::Null); 
                         }
                     }
                }

                let buffer = BUFFER.lock().unwrap();
                window.update_with_buffer(&buffer, w, h).map_err(|e| e.to_string())?;
            }
            Ok(Value::Null)
        },
        
        "clear" => {
             let color = args.get(0).and_then(|v| v.as_int().ok()).unwrap_or(0) as u32;
             let mut buf = BUFFER.lock().unwrap();
             for p in buf.iter_mut() { *p = color; }
             Ok(Value::Null)
        },

        "label" => {
             let x = args.get(0).and_then(|v| v.as_int().ok()).unwrap_or(0) as usize;
             let y = args.get(1).and_then(|v| v.as_int().ok()).unwrap_or(0) as usize;
             let text = args.get(2).and_then(|v| match v { Value::Str(s) => Some(s), _ => None }).ok_or("Expected text")?;
             
             let width = *WIDTH.lock().unwrap();
             let height = *HEIGHT.lock().unwrap();
             let mut buf = BUFFER.lock().unwrap();
             
             draw_text(&mut buf, width, height, &text.to_uppercase(), x, y, 0xFFFFFF);
             Ok(Value::Null)
        },

        "button" => {
             let x = args.get(0).and_then(|v| v.as_int().ok()).unwrap_or(0) as usize;
             let y = args.get(1).and_then(|v| v.as_int().ok()).unwrap_or(0) as usize;
             let w = args.get(2).and_then(|v| v.as_int().ok()).unwrap_or(50) as usize;
             let h = args.get(3).and_then(|v| v.as_int().ok()).unwrap_or(20) as usize;
             let text = args.get(4).and_then(|v| match v { Value::Str(s) => Some(s), _ => None }).unwrap_or(&"".to_string()).clone();

             let mut clicked = false;
             {
                 let ui = UI_STATE.lock().unwrap();
                 if ui.mouse_x >= x as f32 && ui.mouse_x <= (x + w) as f32 &&
                    ui.mouse_y >= y as f32 && ui.mouse_y <= (y + h) as f32 {
                     // Hot
                     if ui.mouse_down {
                         clicked = true;
                         // Active logic could go here
                     }
                 }
             }

             let bg_color = if clicked { 0x888888 } else { 0x444444 };
             
             let width = *WIDTH.lock().unwrap();
             let height = *HEIGHT.lock().unwrap();
             let mut buf = BUFFER.lock().unwrap();

             draw_rect(&mut buf, width, height, x, y, w, h, bg_color);
             draw_text(&mut buf, width, height, &text.to_uppercase(), x + 5, y + 5, 0xFFFFFF);

             Ok(Value::Bool(clicked)) // Note: this makes it return true EVERY frame mouse is down. 
             // Ideally we want on_click (release or unique press).
             // But for simple Immediate Mode, is_down is acceptable or we need previous frame state.
             // We'll stick to is_down for simplicity.
        },

        "plot_lines" => {
             // x, y, w, h, values[]
             let x = args.get(0).and_then(|v| v.as_int().ok()).unwrap_or(0) as usize;
             let y = args.get(1).and_then(|v| v.as_int().ok()).unwrap_or(0) as usize;
             let w = args.get(2).and_then(|v| v.as_int().ok()).unwrap_or(100) as usize;
             let h = args.get(3).and_then(|v| v.as_int().ok()).unwrap_or(100) as usize;
             let values = args.get(4).and_then(|v| match v { Value::Array(a) => Some(a), _ => None }).ok_or("Expected array")?;

             let width = *WIDTH.lock().unwrap();
             let height = *HEIGHT.lock().unwrap();
             let mut buf = BUFFER.lock().unwrap();
             
             // Draw BG
             draw_rect(&mut buf, width, height, x, y, w, h, 0x111111);

             if values.len() < 2 { return Ok(Value::Null); }

             let step_x = w as f32 / (values.len() - 1) as f32;
             let max_val = values.iter().map(|v| v.as_float().unwrap_or(0.0)).fold(0.0/0.0, f64::max) as f32;
             let min_val = values.iter().map(|v| v.as_float().unwrap_or(0.0)).fold(0.0/0.0, f64::min) as f32;
             let range = if (max_val - min_val).abs() < 0.001 { 1.0 } else { max_val - min_val };

             let mut prev_px = x;
             let mut prev_py = y + h - ((values[0].as_float().unwrap_or(0.0) as f32 - min_val) / range * h as f32) as usize;

             for i in 1..values.len() {
                 let px = x + (i as f32 * step_x) as usize;
                 let val = values[i].as_float().unwrap_or(0.0) as f32;
                 let py = y + h - ((val - min_val) / range * h as f32) as usize;
                 
                 // Draw line from prev to current (Naive H/V steps or simple Bresenham if desired)
                 // Simple dots for now to save space or simple H-V? 
                 // Let's do simple naive line (lerp y)
                 let dx = px as isize - prev_px as isize;
                 if dx > 0 {
                     for ix in 0..dx {
                         let cx = prev_px + ix as usize;
                         let t = ix as f32 / dx as f32;
                         let cy = prev_py as f32 + (py as f32 - prev_py as f32) * t;
                         if cx < width && (cy as usize) < height {
                             buf[cy as usize * width + cx] = 0x00FF00;
                         }
                     }
                 }
                 prev_px = px;
                 prev_py = py;
             }

             Ok(Value::Null)
        },

        "draw_line" => {
             let x1 = args.get(0).and_then(|v| v.as_int().ok()).unwrap_or(0) as isize;
             let y1 = args.get(1).and_then(|v| v.as_int().ok()).unwrap_or(0) as isize;
             let x2 = args.get(2).and_then(|v| v.as_int().ok()).unwrap_or(0) as isize;
             let y2 = args.get(3).and_then(|v| v.as_int().ok()).unwrap_or(0) as isize;
             let color = args.get(4).and_then(|v| v.as_int().ok()).unwrap_or(0xFFFFFF) as u32;

             let width = *WIDTH.lock().unwrap() as isize;
             let height = *HEIGHT.lock().unwrap() as isize;
             let mut buf = BUFFER.lock().unwrap();

             let dx = (x2 - x1).abs();
             let dy = -(y2 - y1).abs();
             let sx = if x1 < x2 { 1 } else { -1 };
             let sy = if y1 < y2 { 1 } else { -1 };
             let mut err = dx + dy;
             let mut cx = x1;
             let mut cy = y1;

             loop {
                 if cx >= 0 && cx < width && cy >= 0 && cy < height {
                     buf[(cy * width + cx) as usize] = color;
                 }
                 if cx == x2 && cy == y2 { break; }
                 let e2 = 2 * err;
                 if e2 >= dy { err += dy; cx += sx; }
                 if e2 <= dx { err += dx; cy += sy; }
             }
             Ok(Value::Null)
        },
        "draw_circle" => {
             let cx = args.get(0).and_then(|v| v.as_int().ok()).unwrap_or(0) as isize;
             let cy = args.get(1).and_then(|v| v.as_int().ok()).unwrap_or(0) as isize;
             let r = args.get(2).and_then(|v| v.as_int().ok()).unwrap_or(0) as isize;
             let color = args.get(3).and_then(|v| v.as_int().ok()).unwrap_or(0xFFFFFF) as u32;

             let width = *WIDTH.lock().unwrap() as isize;
             let height = *HEIGHT.lock().unwrap() as isize;
             let mut buf = BUFFER.lock().unwrap();

             for y in -r..=r {
                 for x in -r..=r {
                     if x*x + y*y <= r*r {
                         let px = cx + x;
                         let py = cy + y;
                         if px >= 0 && px < width && py >= 0 && py < height {
                             buf[(py * width + px) as usize] = color;
                         }
                     }
                 }
             }
             Ok(Value::Null)
        },

        _ => Err(format!("Unknown graph function: {}", name))
    }
}
