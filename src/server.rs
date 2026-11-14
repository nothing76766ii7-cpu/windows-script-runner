//! Embedded Local HTTP Server (`127.0.0.1:18765`) & Raw Android JNI Bridge (`Java_com_vanta_wsr_RustBridge_*`).
//! Zero external crate dependencies — works both inside Android APK WebView and desktop/Termux browsers.

use crate::mcp::{extract_json_str, handle_mcp_call, json_escape};
use crate::vfs::WinVfs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::raw::{c_char, c_void};
use std::thread;

pub fn start_http_server(port: u16) -> Result<(), String> {
    let vfs = WinVfs::new();
    let _ = vfs.ensure_initialized();
    let addr = format!("127.0.0.1:{}", port);
    let listener = TcpListener::bind(&addr).map_err(|e| format!("Failed binding {}: {}", addr, e))?;
    for stream in listener.incoming().flatten() {
        thread::spawn(move || {
            let _ = handle_client(stream);
        });
    }
    Ok(())
}

fn handle_client(mut stream: TcpStream) -> Result<(), std::io::Error> {
    let mut buf = [0u8; 65536];
    let n = stream.read(&mut buf)?;
    if n == 0 {
        return Ok(());
    }
    let req = String::from_utf8_lossy(&buf[..n]);
    let first_line = req.lines().next().unwrap_or("");

    if first_line.starts_with("OPTIONS ") {
        let resp = "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: POST, GET, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\n\r\n";
        stream.write_all(resp.as_bytes())?;
        return Ok(());
    }

    let body = if let Some(idx) = req.find("\r\n\r\n") {
        &req[idx + 4..]
    } else if let Some(idx) = req.find("\n\n") {
        &req[idx + 2..]
    } else {
        ""
    };

    let tool = extract_json_str(body, "tool").unwrap_or_else(|| "wsr_check_environment".to_string());
    let json_resp = handle_mcp_call(&tool, body);

    let http_resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\n\r\n{}",
        json_resp.len(),
        json_resp
    );
    stream.write_all(http_resp.as_bytes())?;
    stream.flush()?;
    Ok(())
}

// ─── ZERO-DEPENDENCY ANDROID JNI C-ABI TABLE DEFINITIONS ────────────────────
#[repr(C)]
pub struct JNINativeInterface {
    pub reserved0: *mut c_void,
    pub reserved1: *mut c_void,
    pub reserved2: *mut c_void,
    pub reserved3: *mut c_void,
    pub get_version: Option<unsafe extern "system" fn(env: *mut JNIEnv) -> i32>,
}

pub type JNIEnv = *const JNINativeInterface;
pub type JObject = *mut c_void;
pub type JString = *mut c_void;

static mut SERVER_STARTED: bool = false;

/// Starts the background Rust VFS & script execution HTTP server on port 18765 when loaded by the Android APK.
#[no_mangle]
pub unsafe extern "system" fn Java_com_vanta_wsr_RustBridge_startServer(
    _env: *mut JNIEnv,
    _class: JObject,
    port: i32,
) -> i32 {
    if SERVER_STARTED {
        return port;
    }
    SERVER_STARTED = true;
    let p = if port <= 0 { 18765 } else { port as u16 };
    thread::spawn(move || {
        let _ = start_http_server(p);
    });
    p as i32
}

/// Initializes the Windows Virtual File System (`C:\`, `D:\`, `Z:\`) inside the Android app's storage directory.
#[no_mangle]
pub unsafe extern "system" fn Java_com_vanta_wsr_RustBridge_initVfs(
    _env: *mut JNIEnv,
    _class: JObject,
) -> i32 {
    let vfs = WinVfs::new();
    match vfs.ensure_initialized() {
        Ok(_) => 0,
        Err(_) => 1,
    }
}

/// Optional C-ABI entrypoint for FFI callers passing null-terminated UTF-8 JSON requests.
#[no_mangle]
pub unsafe extern "C" fn wsr_invoke_json(req_ptr: *const c_char, out_buf: *mut u8, out_cap: usize) -> usize {
    if req_ptr.is_null() || out_buf.is_null() || out_cap == 0 {
        return 0;
    }
    let c_str = std::ffi::CStr::from_ptr(req_ptr);
    let req = c_str.to_string_lossy();
    let tool = extract_json_str(&req, "tool").unwrap_or_else(|| "wsr_check_environment".to_string());
    let resp = handle_mcp_call(&tool, &req);
    let bytes = resp.as_bytes();
    let copy_len = bytes.len().min(out_cap - 1);
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), out_buf, copy_len);
    *out_buf.add(copy_len) = 0;
    copy_len
}

pub fn format_health_json() -> String {
    let vfs = WinVfs::new();
    format!(
        "{{\"status\":\"ok\",\"engine\":\"rust-3.0\",\"drive_c\":\"{}\",\"drive_d\":\"{}\"}}",
        json_escape(&vfs.drive_c.to_string_lossy()),
        json_escape(&vfs.drive_d.to_string_lossy())
    )
}
