/// Networking module for KiVM — Build 28
/// Provides TCP, UDP and HTTP networking primitives.
/// TCP/UDP use std::net (synchronous, ownership-safe).
/// HTTP uses ureq (synchronous blocking).

use crate::vm::Value;
use std::collections::HashMap;
use std::io::{Read, Write, BufRead, BufReader};
use std::net::{TcpStream, TcpListener, UdpSocket, Shutdown};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// --- Connection Registry ---
// Connections are stored in a global registry keyed by integer IDs.
// This avoids exposing raw pointers/handles to the Kinetix language layer.

lazy_static::lazy_static! {
    static ref TCP_STREAMS: Arc<Mutex<HashMap<i64, TcpStream>>> = Arc::new(Mutex::new(HashMap::new()));
    static ref TCP_LISTENERS: Arc<Mutex<HashMap<i64, TcpListener>>> = Arc::new(Mutex::new(HashMap::new()));
    static ref UDP_SOCKETS: Arc<Mutex<HashMap<i64, UdpSocket>>> = Arc::new(Mutex::new(HashMap::new()));
    static ref NEXT_CONN_ID: Arc<Mutex<i64>> = Arc::new(Mutex::new(1));
}

fn next_id() -> Result<i64, String> {
    let mut id = NEXT_CONN_ID.lock().map_err(|_| "Connection ID lock failed".to_string())?;
    let current = *id;
    *id += 1;
    Ok(current)
}

/// Helper: build a Result<T,E> map for Kinetix
fn ok_result(val: Value) -> Value {
    let mut m = HashMap::new();
    m.insert("ok".to_string(), val);
    Value::Map(m)
}

fn err_result(msg: &str) -> Value {
    let mut m = HashMap::new();
    m.insert("err".to_string(), Value::Str(msg.to_string()));
    Value::Map(m)
}

pub fn call(func_name: &str, args: &[Value]) -> Result<Value, String> {
    match func_name {
        // =====================================================================
        // TCP
        // =====================================================================

        // net.tcp.connect(addr, port) -> Result<Connection, E>
        "tcp.connect" => {
            let addr = match args.get(0) {
                Some(Value::Str(s)) => s.clone(),
                _ => return Ok(err_result("Expected address string")),
            };
            let port = match args.get(1) {
                Some(Value::Int(p)) => *p as u16,
                _ => return Ok(err_result("Expected port integer")),
            };
            let target = format!("{}:{}", addr, port);
            match TcpStream::connect(&target) {
                Ok(stream) => {
                    let id = next_id()?;
                    TCP_STREAMS.lock().map_err(|_| "TCP lock failed".to_string())?.insert(id, stream);
                    Ok(ok_result(Value::Int(id)))
                }
                Err(e) => Ok(err_result(&format!("TCP connect failed: {}", e))),
            }
        }

        // net.tcp.listen(port) -> Result<Listener, E>
        "tcp.listen" => {
            let port = match args.get(0) {
                Some(Value::Int(p)) => *p as u16,
                _ => return Ok(err_result("Expected port integer")),
            };
            let bind_addr = format!("0.0.0.0:{}", port);
            match TcpListener::bind(&bind_addr) {
                Ok(listener) => {
                    let id = next_id()?;
                    TCP_LISTENERS.lock().map_err(|_| "TCP lock failed".to_string())?.insert(id, listener);
                    Ok(ok_result(Value::Int(id)))
                }
                Err(e) => Ok(err_result(&format!("TCP listen failed: {}", e))),
            }
        }

        // net.tcp.accept(listener_id) -> Result<Connection, E>
        "tcp.accept" => {
            let lid = match args.get(0) {
                Some(Value::Int(id)) => *id,
                _ => return Ok(err_result("Expected listener ID")),
            };
            let listeners = TCP_LISTENERS.lock().map_err(|_| "TCP lock failed".to_string())?;
            let listener = listeners.get(&lid).ok_or_else(|| format!("Listener {} not found", lid))?;
            match listener.accept() {
                Ok((stream, addr)) => {
                    drop(listeners); // release lock before acquiring next
                    let id = next_id()?;
                    TCP_STREAMS.lock().map_err(|_| "TCP lock failed".to_string())?.insert(id, stream);
                    let mut res = HashMap::new();
                    res.insert("ok".to_string(), Value::Int(id));
                    res.insert("addr".to_string(), Value::Str(addr.to_string()));
                    Ok(Value::Map(res))
                }
                Err(e) => Ok(err_result(&format!("TCP accept failed: {}", e))),
            }
        }

        // net.tcp.send(conn_id, data) -> Result<(), E>
        "tcp.send" => {
            let cid = match args.get(0) {
                Some(Value::Int(id)) => *id,
                _ => return Ok(err_result("Expected connection ID")),
            };
            let data = match args.get(1) {
                Some(Value::Str(s)) => s.as_bytes().to_vec(),
                _ => return Ok(err_result("Expected data string")),
            };
            let mut streams = TCP_STREAMS.lock().map_err(|_| "TCP lock failed".to_string())?;
            let stream = streams.get_mut(&cid).ok_or_else(|| format!("Connection {} not found", cid))?;
            match stream.write_all(&data) {
                Ok(_) => Ok(ok_result(Value::Null)),
                Err(e) => Ok(err_result(&format!("TCP send failed: {}", e))),
            }
        }

        // net.tcp.recv(conn_id, max_bytes?) -> Result<String, E>
        "tcp.recv" => {
            let cid = match args.get(0) {
                Some(Value::Int(id)) => *id,
                _ => return Ok(err_result("Expected connection ID")),
            };
            let max_bytes = match args.get(1) {
                Some(Value::Int(n)) => *n as usize,
                _ => 4096,
            };
            let mut streams = TCP_STREAMS.lock().map_err(|_| "TCP lock failed".to_string())?;
            let stream = streams.get_mut(&cid).ok_or_else(|| format!("Connection {} not found", cid))?;
            let mut buf = vec![0u8; max_bytes];
            match stream.read(&mut buf) {
                Ok(n) => {
                    let text = String::from_utf8_lossy(&buf[..n]).to_string();
                    Ok(ok_result(Value::Str(text)))
                }
                Err(e) => Ok(err_result(&format!("TCP recv failed: {}", e))),
            }
        }

        // net.tcp.recvLine(conn_id) -> Result<String, E>
        "tcp.recvLine" => {
            let cid = match args.get(0) {
                Some(Value::Int(id)) => *id,
                _ => return Ok(err_result("Expected connection ID")),
            };
            let mut streams = TCP_STREAMS.lock().map_err(|_| "TCP lock failed".to_string())?;
            let stream = streams.get_mut(&cid).ok_or_else(|| format!("Connection {} not found", cid))?;
            let cloned = stream.try_clone().map_err(|e| e.to_string())?;
            drop(streams);
            let mut reader = BufReader::new(cloned);
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(_) => Ok(ok_result(Value::Str(line.trim_end().to_string()))),
                Err(e) => Ok(err_result(&format!("TCP recvLine failed: {}", e))),
            }
        }

        // net.tcp.setTimeout(conn_id, millis) -> Result<(), E>
        "tcp.setTimeout" => {
            let cid = match args.get(0) {
                Some(Value::Int(id)) => *id,
                _ => return Ok(err_result("Expected connection ID")),
            };
            let ms = match args.get(1) {
                Some(Value::Int(n)) => *n as u64,
                _ => return Ok(err_result("Expected timeout in milliseconds")),
            };
            let streams = TCP_STREAMS.lock().map_err(|_| "TCP lock failed".to_string())?;
            let stream = streams.get(&cid).ok_or_else(|| format!("Connection {} not found", cid))?;
            let dur = Some(Duration::from_millis(ms));
            stream.set_read_timeout(dur).map_err(|e| e.to_string())?;
            stream.set_write_timeout(dur).map_err(|e| e.to_string())?;
            Ok(ok_result(Value::Null))
        }

        // net.tcp.setNoDelay(conn_id, bool) -> Result<(), E>
        "tcp.setNoDelay" => {
            let cid = match args.get(0) {
                Some(Value::Int(id)) => *id,
                _ => return Ok(err_result("Expected connection ID")),
            };
            let no_delay = match args.get(1) {
                Some(Value::Bool(b)) => *b,
                _ => true,
            };
            let streams = TCP_STREAMS.lock().map_err(|_| "TCP lock failed".to_string())?;
            let stream = streams.get(&cid).ok_or_else(|| format!("Connection {} not found", cid))?;
            stream.set_nodelay(no_delay).map_err(|e| e.to_string())?;
            Ok(ok_result(Value::Null))
        }

        // net.tcp.shutdown(conn_id) -> Result<(), E>
        "tcp.shutdown" => {
            let cid = match args.get(0) {
                Some(Value::Int(id)) => *id,
                _ => return Ok(err_result("Expected connection ID")),
            };
            let mut streams = TCP_STREAMS.lock().map_err(|_| "TCP lock failed".to_string())?;
            if let Some(stream) = streams.remove(&cid) {
                let _ = stream.shutdown(Shutdown::Both);
                Ok(ok_result(Value::Null))
            } else {
                Ok(err_result(&format!("Connection {} not found", cid)))
            }
        }

        // net.tcp.close(conn_id) — alias for shutdown
        "tcp.close" => call("tcp.shutdown", args),

        // net.tcp.localAddr(conn_id) -> Result<String, E>
        "tcp.localAddr" => {
            let cid = match args.get(0) {
                Some(Value::Int(id)) => *id,
                _ => return Ok(err_result("Expected connection ID")),
            };
            let streams = TCP_STREAMS.lock().map_err(|_| "TCP lock failed".to_string())?;
            let stream = streams.get(&cid).ok_or_else(|| format!("Connection {} not found", cid))?;
            Ok(ok_result(Value::Str(stream.local_addr().map(|a| a.to_string()).unwrap_or_default())))
        }

        // net.tcp.peerAddr(conn_id) -> Result<String, E>
        "tcp.peerAddr" => {
            let cid = match args.get(0) {
                Some(Value::Int(id)) => *id,
                _ => return Ok(err_result("Expected connection ID")),
            };
            let streams = TCP_STREAMS.lock().map_err(|_| "TCP lock failed".to_string())?;
            let stream = streams.get(&cid).ok_or_else(|| format!("Connection {} not found", cid))?;
            Ok(ok_result(Value::Str(stream.peer_addr().map(|a| a.to_string()).unwrap_or_default())))
        }

        // =====================================================================
        // UDP
        // =====================================================================

        // net.udp.bind(port) -> Result<Socket, E>
        "udp.bind" => {
            let port = match args.get(0) {
                Some(Value::Int(p)) => *p as u16,
                _ => return Ok(err_result("Expected port integer")),
            };
            let bind_addr = format!("0.0.0.0:{}", port);
            match UdpSocket::bind(&bind_addr) {
                Ok(socket) => {
                    let id = next_id()?;
                    UDP_SOCKETS.lock().map_err(|_| "UDP lock failed".to_string())?.insert(id, socket);
                    Ok(ok_result(Value::Int(id)))
                }
                Err(e) => Ok(err_result(&format!("UDP bind failed: {}", e))),
            }
        }

        // net.udp.send(socket_id, addr, port, data) -> Result<usize, E>
        "udp.send" => {
            let sid = match args.get(0) {
                Some(Value::Int(id)) => *id,
                _ => return Ok(err_result("Expected socket ID")),
            };
            let addr = match args.get(1) {
                Some(Value::Str(s)) => s.clone(),
                _ => return Ok(err_result("Expected address string")),
            };
            let port = match args.get(2) {
                Some(Value::Int(p)) => *p as u16,
                _ => return Ok(err_result("Expected port integer")),
            };
            let data = match args.get(3) {
                Some(Value::Str(s)) => s.as_bytes().to_vec(),
                _ => return Ok(err_result("Expected data string")),
            };
            let sockets = UDP_SOCKETS.lock().map_err(|_| "UDP lock failed".to_string())?;
            let sock = sockets.get(&sid).ok_or_else(|| format!("Socket {} not found", sid))?;
            let target = format!("{}:{}", addr, port);
            match sock.send_to(&data, &target) {
                Ok(n) => Ok(ok_result(Value::Int(n as i64))),
                Err(e) => Ok(err_result(&format!("UDP send failed: {}", e))),
            }
        }

        // net.udp.recv(socket_id, max_bytes?) -> Result<{data, addr}, E>
        "udp.recv" => {
            let sid = match args.get(0) {
                Some(Value::Int(id)) => *id,
                _ => return Ok(err_result("Expected socket ID")),
            };
            let max_bytes = match args.get(1) {
                Some(Value::Int(n)) => *n as usize,
                _ => 4096,
            };
            let sockets = UDP_SOCKETS.lock().map_err(|_| "UDP lock failed".to_string())?;
            let sock = sockets.get(&sid).ok_or_else(|| format!("Socket {} not found", sid))?;
            let mut buf = vec![0u8; max_bytes];
            match sock.recv_from(&mut buf) {
                Ok((n, addr)) => {
                    let mut res = HashMap::new();
                    res.insert("ok".to_string(), Value::Str(String::from_utf8_lossy(&buf[..n]).to_string()));
                    res.insert("addr".to_string(), Value::Str(addr.to_string()));
                    Ok(Value::Map(res))
                }
                Err(e) => Ok(err_result(&format!("UDP recv failed: {}", e))),
            }
        }

        // net.udp.setTimeout(socket_id, millis) -> Result<(), E>
        "udp.setTimeout" => {
            let sid = match args.get(0) {
                Some(Value::Int(id)) => *id,
                _ => return Ok(err_result("Expected socket ID")),
            };
            let ms = match args.get(1) {
                Some(Value::Int(n)) => *n as u64,
                _ => return Ok(err_result("Expected timeout in milliseconds")),
            };
            let sockets = UDP_SOCKETS.lock().map_err(|_| "UDP lock failed".to_string())?;
            let sock = sockets.get(&sid).ok_or_else(|| format!("Socket {} not found", sid))?;
            let dur = Some(Duration::from_millis(ms));
            sock.set_read_timeout(dur).map_err(|e| e.to_string())?;
            sock.set_write_timeout(dur).map_err(|e| e.to_string())?;
            Ok(ok_result(Value::Null))
        }

        // net.udp.close(socket_id) -> Result<(), E>
        "udp.close" => {
            let sid = match args.get(0) {
                Some(Value::Int(id)) => *id,
                _ => return Ok(err_result("Expected socket ID")),
            };
            let mut sockets = UDP_SOCKETS.lock().map_err(|_| "UDP lock failed".to_string())?;
            if sockets.remove(&sid).is_some() {
                Ok(ok_result(Value::Null))
            } else {
                Ok(err_result(&format!("Socket {} not found", sid)))
            }
        }

        // =====================================================================
        // HTTP (existing, via ureq — synchronous)
        // =====================================================================

        "get" | "http.get" => {
            if let Some(Value::Str(url)) = args.first() {
                match ureq::get(url).call() {
                    Ok(resp) => {
                        let status = resp.status();
                        let text = resp.into_string().map_err(|e| e.to_string())?;
                        let mut res = HashMap::new();
                        res.insert("ok".to_string(), Value::Str(text));
                        res.insert("status".to_string(), Value::Int(status as i64));
                        Ok(Value::Map(res))
                    }
                    Err(e) => Ok(err_result(&format!("HTTP GET failed: {}", e))),
                }
            } else {
                Ok(err_result("Expected URL string"))
            }
        }

        "post" | "http.post" => {
            if let (Some(Value::Str(url)), Some(body_val)) = (args.get(0), args.get(1)) {
                let body_str = format!("{}", body_val);
                match ureq::post(url).send_string(&body_str) {
                    Ok(resp) => {
                        let status = resp.status();
                        let text = resp.into_string().map_err(|e| e.to_string())?;
                        let mut res = HashMap::new();
                        res.insert("ok".to_string(), Value::Str(text));
                        res.insert("status".to_string(), Value::Int(status as i64));
                        Ok(Value::Map(res))
                    }
                    Err(e) => Ok(err_result(&format!("HTTP POST failed: {}", e))),
                }
            } else {
                Ok(err_result("Expected URL and body"))
            }
        }

        "download" | "http.download" => {
            if let (Some(Value::Str(url)), Some(Value::Str(dest))) = (args.get(0), args.get(1)) {
                match ureq::get(url).call() {
                    Ok(resp) => {
                        let mut reader = resp.into_reader();
                        let mut file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
                        std::io::copy(&mut reader, &mut file).map_err(|e| e.to_string())?;
                        Ok(ok_result(Value::Null))
                    }
                    Err(e) => Ok(err_result(&format!("Download failed: {}", e))),
                }
            } else {
                Ok(err_result("Expected URL and destination path"))
            }
        }

        // =====================================================================
        // Utility
        // =====================================================================

        // net.resolve(hostname) -> Result<String, E>
        "resolve" => {
            if let Some(Value::Str(host)) = args.first() {
                use std::net::ToSocketAddrs;
                let lookup = format!("{}:0", host);
                match lookup.to_socket_addrs() {
                    Ok(mut addrs) => {
                        if let Some(addr) = addrs.next() {
                            Ok(ok_result(Value::Str(addr.ip().to_string())))
                        } else {
                            Ok(err_result("No addresses found"))
                        }
                    }
                    Err(e) => Ok(err_result(&format!("DNS resolution failed: {}", e))),
                }
            } else {
                Ok(err_result("Expected hostname string"))
            }
        }

        // net.ping(address, timeout_ms) -> Result<u32, E>
        // Uses TCP connect probe (port 80) as ICMP requires root/admin privileges.
        // Returns round-trip time in milliseconds.
        "ping" => {
            let addr = match args.get(0) {
                Some(Value::Str(s)) => s.clone(),
                _ => return Ok(err_result("Expected address string")),
            };
            let timeout_ms = match args.get(1) {
                Some(Value::Int(n)) => *n as u64,
                _ => 1000, // default 1 second
            };

            // Resolve hostname first if not an IP
            use std::net::ToSocketAddrs;
            let target = format!("{}:80", addr);
            let resolved = match target.to_socket_addrs() {
                Ok(mut addrs) => match addrs.next() {
                    Some(a) => a,
                    None => return Ok(err_result("Could not resolve address")),
                },
                Err(e) => return Ok(err_result(&format!("DNS resolution failed: {}", e))),
            };

            let start = std::time::Instant::now();
            match TcpStream::connect_timeout(&resolved, Duration::from_millis(timeout_ms)) {
                Ok(stream) => {
                    let elapsed = start.elapsed().as_millis() as i64;
                    let _ = stream.shutdown(Shutdown::Both);
                    Ok(ok_result(Value::Int(elapsed)))
                }
                Err(e) => Ok(err_result(&format!("Ping failed: {}", e))),
            }
        }

        // net.getInterfaces() -> List of Maps {name, addr}
        // Enumerates local network interfaces via OS commands.
        "getInterfaces" => {
            let mut interfaces: Vec<Value> = Vec::new();

            #[cfg(target_os = "windows")]
            {
                // Use ipconfig on Windows
                if let Ok(output) = std::process::Command::new("ipconfig").output() {
                    let text = String::from_utf8_lossy(&output.stdout);
                    let mut current_name = String::new();
                    for line in text.lines() {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() && !trimmed.starts_with(' ') && line.ends_with(':') {
                            current_name = trimmed.trim_end_matches(':').to_string();
                        }
                        if let Some(ip_part) = trimmed.strip_prefix("IPv4 Address") {
                            if let Some(ip) = ip_part.split(':').last() {
                                let ip = ip.trim();
                                let mut m = HashMap::new();
                                m.insert("name".to_string(), Value::Str(current_name.clone()));
                                m.insert("addr".to_string(), Value::Str(ip.to_string()));
                                interfaces.push(Value::Map(m));
                            }
                        }
                        // Also catch "Indirizzo IPv4" for Italian locale
                        if let Some(ip_part) = trimmed.strip_prefix("Indirizzo IPv4") {
                            if let Some(ip) = ip_part.split(':').last() {
                                let ip = ip.trim();
                                let mut m = HashMap::new();
                                m.insert("name".to_string(), Value::Str(current_name.clone()));
                                m.insert("addr".to_string(), Value::Str(ip.to_string()));
                                interfaces.push(Value::Map(m));
                            }
                        }
                    }
                }
            }

            #[cfg(target_os = "linux")]
            {
                if let Ok(output) = std::process::Command::new("ip").args(&["-o", "-4", "addr", "show"]).output() {
                    let text = String::from_utf8_lossy(&output.stdout);
                    for line in text.lines() {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 4 {
                            let name = parts[1].to_string();
                            let addr_cidr = parts[3];
                            let addr = addr_cidr.split('/').next().unwrap_or("").to_string();
                            let mut m = HashMap::new();
                            m.insert("name".to_string(), Value::Str(name));
                            m.insert("addr".to_string(), Value::Str(addr));
                            interfaces.push(Value::Map(m));
                        }
                    }
                }
            }

            #[cfg(target_os = "macos")]
            {
                if let Ok(output) = std::process::Command::new("ifconfig").output() {
                    let text = String::from_utf8_lossy(&output.stdout);
                    let mut current_name = String::new();
                    for line in text.lines() {
                        // Interface header lines don't start with whitespace
                        if !line.starts_with('\t') && !line.starts_with(' ') && line.contains(':') {
                            current_name = line.split(':').next().unwrap_or("").to_string();
                        }
                        let trimmed = line.trim();
                        if trimmed.starts_with("inet ") && !trimmed.starts_with("inet6") {
                            let parts: Vec<&str> = trimmed.split_whitespace().collect();
                            if parts.len() >= 2 {
                                let mut m = HashMap::new();
                                m.insert("name".to_string(), Value::Str(current_name.clone()));
                                m.insert("addr".to_string(), Value::Str(parts[1].to_string()));
                                interfaces.push(Value::Map(m));
                            }
                        }
                    }
                }
            }

            Ok(Value::Array(interfaces))
        }

        // net.tls.connect(addr, port) -> Result<Connection, E>
        // Placeholder for Build 30 — TLS support will be expanded in later builds
        "tls.connect" => {
            let addr = match args.get(0) {
                Some(Value::Str(s)) => s.clone(),
                _ => return Ok(err_result("Expected address string")),
            };
            let port = match args.get(1) {
                Some(Value::Int(p)) => *p as u16,
                _ => 443,
            };
            // For now, use ureq's TLS internally via a simple HTTPS GET probe
            // Full TLS socket support planned for future builds
            Ok(err_result(&format!("TLS socket support for {}:{} is planned for a future build. Use net.http.get with https:// URLs for TLS connections.", addr, port)))
        }

        _ => Err(format!("Unknown net function: {}", func_name)),
    }
}
