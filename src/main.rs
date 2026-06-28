use std::collections::HashMap;
use std::env;
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

const MAX_BODY_BYTES: usize = 16 * 1024;

#[derive(Clone)]
struct Song {
    id: u64,
    title: String,
    requester: String,
    done: bool,
}
#[derive(Default)]
struct AppState {
    songs: Vec<Song>,
    next_song_id: u64,
}

fn main() -> std::io::Result<()> {
    let port = env::var("PORT").unwrap_or_else(|_| "7878".to_string());
    let listener = TcpListener::bind(format!("0.0.0.0:{port}"))?;
    let state = Arc::new(Mutex::new(AppState::default()));
    println!("Classroom Song Link: http://localhost:{port}/");
    for stream in listener.incoming() {
        let state = Arc::clone(&state);
        thread::spawn(move || {
            if let Ok(stream) = stream {
                let _ = handle_client(stream, state);
            }
        });
    }
    Ok(())
}

fn handle_client(mut stream: TcpStream, state: Arc<Mutex<AppState>>) -> std::io::Result<()> {
    let mut buffer = [0_u8; 8192];
    let bytes_read = stream.read(&mut buffer)?;
    if bytes_read == 0 {
        return Ok(());
    }
    let request = String::from_utf8_lossy(&buffer[..bytes_read]);
    let Some((header, body_start)) = request.split_once("\r\n\r\n") else {
        return text_response(&mut stream, 400, "Bad request");
    };
    let mut lines = header.lines();
    let parts: Vec<&str> = lines
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .collect();
    if parts.len() < 2 {
        return text_response(&mut stream, 400, "Bad request");
    }
    let method = parts[0];
    let target = parts[1];
    let (path, _) = target.split_once('?').unwrap_or((target, ""));
    let headers = parse_headers(lines);
    let length = headers
        .get("content-length")
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0)
        .min(MAX_BODY_BYTES);
    let mut body = body_start.as_bytes().to_vec();
    while body.len() < length {
        let mut chunk = [0_u8; 4096];
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(length);
    let body = String::from_utf8_lossy(&body);

    match (method, path) {
        ("GET", "/health") => text_response(&mut stream, 200, "ok"),
        ("GET", "/") => public_page(&mut stream, &headers),
        ("GET", "/admin") => html_response(&mut stream, include_str!("../web/admin.html")),
        ("GET", "/api/songs") => global_songs_api(&mut stream, state),
        ("POST", "/api/request") => add_global_song(&mut stream, &body, state),
        ("POST", "/api/done") => mark_global_done(&mut stream, &body, state),
        ("POST", "/api/clear") => clear_done_songs(&mut stream, state),
        _ => text_response(&mut stream, 404, "Not found"),
    }
}

fn public_page(stream: &mut TcpStream, headers: &HashMap<String, String>) -> std::io::Result<()> {
    let page = include_str!("../web/public.html")
        .replace("__SHARE_URL__", &format!("{}/", public_base_url(headers)));
    html_response(stream, &page)
}

fn global_songs_api(
    stream: &mut TcpStream,
    state: Arc<Mutex<AppState>>,
) -> std::io::Result<()> {
    let state = state.lock().expect("state poisoned");
    let songs: Vec<String> = state
        .songs
        .iter()
        .map(|song| {
            format!(
                r#"{{"id":{},"title":{},"requester":{},"done":{}}}"#,
                song.id,
                json_string(&song.title),
                json_string(&song.requester),
                song.done
            )
        })
        .collect();
    json_response(stream, 200, &format!("[{}]", songs.join(",")))
}

fn add_global_song(
    stream: &mut TcpStream,
    body: &str,
    state: Arc<Mutex<AppState>>,
) -> std::io::Result<()> {
    let params = parse_form(body);
    let title = params.get("title").map(|s| s.trim()).unwrap_or_default();
    if title.is_empty() {
        return json_response(stream, 400, r#"{"error":"请填写歌曲。"}"#);
    }
    let requester = params
        .get("requester")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("匿名");
    let mut state = state.lock().expect("state poisoned");
    state.next_song_id += 1;
    let song = Song {
        id: state.next_song_id,
        title: title.chars().take(160).collect(),
        requester: requester.chars().take(40).collect(),
        done: false,
    };
    state.songs.push(song.clone());
    json_response(
        stream,
        201,
        &format!(
            r#"{{"id":{},"title":{}}}"#,
            song.id,
            json_string(&song.title)
        ),
    )
}

fn mark_global_done(
    stream: &mut TcpStream,
    body: &str,
    state: Arc<Mutex<AppState>>,
) -> std::io::Result<()> {
    let id = parse_form(body)
        .get("id")
        .and_then(|s| s.parse::<u64>().ok());
    let mut state = state.lock().expect("state poisoned");
    if let Some(id) = id {
        if let Some(song) = state.songs.iter_mut().find(|song| song.id == id) {
            song.done = true;
        }
    }
    json_response(stream, 200, r#"{"ok":true}"#)
}

fn clear_done_songs(
    stream: &mut TcpStream,
    state: Arc<Mutex<AppState>>,
) -> std::io::Result<()> {
    state
        .lock()
        .expect("state poisoned")
        .songs
        .retain(|song| !song.done);
    json_response(stream, 200, r#"{"ok":true}"#)
}

fn parse_form(input: &str) -> HashMap<String, String> {
    input
        .split('&')
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            Some((percent_decode(key)?, percent_decode(value)?))
        })
        .collect()
}
fn parse_headers<'a>(lines: impl Iterator<Item = &'a str>) -> HashMap<String, String> {
    lines
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_ascii_lowercase(), value.trim().to_string()))
        })
        .collect()
}
fn public_base_url(headers: &HashMap<String, String>) -> String {
    if let Ok(value) = env::var("PUBLIC_BASE_URL") {
        let value = value.trim().trim_end_matches('/');
        if !value.is_empty() {
            return value.to_string();
        }
    }
    let host = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get("host"))
        .map(|value| value.split(',').next().unwrap_or(value).trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("localhost:7878");
    let proto = headers
        .get("x-forwarded-proto")
        .map(|value| value.split(',').next().unwrap_or(value).trim())
        .filter(|value| *value == "https" || *value == "http")
        .unwrap_or("http");
    format!("{proto}://{host}")
}
fn percent_decode(input: &str) -> Option<String> {
    let input = input.replace('+', " ");
    let bytes = input.as_bytes();
    let mut output = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            output.push(
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?, 16).ok()?,
            );
            i += 3
        } else {
            output.push(bytes[i]);
            i += 1
        }
    }
    String::from_utf8(output).ok()
}
fn json_string(input: &str) -> String {
    let mut out = String::from("\"");
    for c in input.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
fn html_response(stream: &mut TcpStream, body: &str) -> std::io::Result<()> {
    response(stream, 200, "text/html; charset=utf-8", body)
}
fn json_response(stream: &mut TcpStream, status: u16, body: &str) -> std::io::Result<()> {
    response(stream, status, "application/json; charset=utf-8", body)
}
fn text_response(stream: &mut TcpStream, status: u16, body: &str) -> std::io::Result<()> {
    response(stream, status, "text/plain; charset=utf-8", body)
}
fn response(stream: &mut TcpStream, status: u16, kind: &str, body: &str) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        201 => "Created",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Error",
    };
    stream.write_all(format!("HTTP/1.1 {status} {reason}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).as_bytes())?;
    stream.write_all(body.as_bytes())
}
