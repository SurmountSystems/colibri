//! Qwen-Image chat: the `qwenimage --serve` line protocol and a PNG for GPUI.
//!
//! This is not the text engine. The child is `qwenimage --model DIR --serve`
//! (or `python3` when the executable ends in `.py`). `COLI_XDNA` is removed
//! from the child environment. `COLI_ENGINE` is not consulted when locating
//! the binary.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const MAX_FRAME_BYTES: u64 = 64 << 20;
const MAX_PROMPT_BYTES: usize = 8192;
const MIN_SIDE: u32 = 256;
const MAX_SIDE: u32 = 2048;
const SIDE_MULTIPLE: u32 = 32;
const MIN_STEPS: u32 = 2;
const MAX_STEPS: u32 = 200;
const MAX_RANDOM_SEED: u32 = 1 << 31;
const REQUEST_ID: &str = "r1";

/// One generate request. An empty seed means the host draws one in `0..2^31`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageGenRequest {
    pub prompt: String,
    pub width: u32,
    pub height: u32,
    pub steps: u32,
    pub seed: Option<u32>,
}

/// PNG bytes plus the numbers the chat card shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageGenResult {
    pub width: u32,
    pub height: u32,
    pub steps: u32,
    pub seed: u64,
    pub png: Vec<u8>,
}

/// Progress the engine reports between READY and IMAGE.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageProgress {
    Queued,
    Encode,
    Denoise { step: u32, steps: u32 },
    Decode,
    Working,
}

pub fn default_image_size() -> (u32, u32) {
    (768, 512)
}

pub fn default_steps() -> u32 {
    8
}

pub fn size_presets() -> &'static [(u32, u32)] {
    &[
        (512, 512),
        (768, 512),
        (512, 768),
        (1024, 576),
        (576, 1024),
        (1024, 1024),
    ]
}

pub fn valid_side(n: u32) -> bool {
    (MIN_SIDE..=MAX_SIDE).contains(&n) && n % SIDE_MULTIPLE == 0
}

pub fn clamp_steps(n: u32) -> u32 {
    n.clamp(MIN_STEPS, MAX_STEPS)
}

/// Search the same install locations as the text engines, for `qwenimage` only.
///
/// Does not read `COLI_ENGINE` or `COLIBRI_ENGINE`.
pub fn locate_qwenimage_executable() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok();
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));
    let candidates = qwenimage_candidates(cwd.as_deref(), exe_dir.as_deref());
    // Linux search is the first existing file. Windows also accepts `qwenimage.exe`.
    #[cfg(windows)]
    {
        for cand in candidates {
            if cand.is_file() {
                return Some(cand);
            }
            let exe = cand.with_extension("exe");
            if exe.is_file() {
                return Some(exe);
            }
        }
        return None;
    }
    #[cfg(not(windows))]
    {
        candidates.into_iter().find(|cand| cand.is_file())
    }
}

fn qwenimage_candidates(cwd: Option<&Path>, exe_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(cwd) = cwd {
        v.push(cwd.join("c").join("qwenimage"));
        v.push(cwd.join("qwenimage"));
    }
    if let Some(dir) = exe_dir {
        v.push(dir.join("qwenimage"));
        v.push(dir.join("libexec").join("colibri").join("qwenimage"));
    }
    v.push(PathBuf::from("/usr/local/libexec/colibri/qwenimage"));
    v.push(PathBuf::from("/usr/libexec/colibri/qwenimage"));
    v
}

fn command_argv(executable: &Path, model_dir: &Path) -> Vec<String> {
    let exe = executable.display().to_string();
    let model = model_dir.display().to_string();
    let mut argv = Vec::new();
    if exe.ends_with(".py") {
        argv.push("python3".to_string());
    }
    argv.push(exe);
    argv.push("--model".to_string());
    argv.push(model);
    argv.push("--serve".to_string());
    argv
}

fn image_child_env_from(
    vars: impl IntoIterator<Item = (String, String)>,
    model: &Path,
) -> HashMap<String, String> {
    let mut env: HashMap<String, String> = vars.into_iter().collect();
    env.insert("SNAP".to_string(), model.display().to_string());
    env.insert("SERVE".to_string(), "1".to_string());
    env.remove("COLI_XDNA");
    env
}

/// Start the image engine, run one GEN, and return a PNG.
pub fn spawn_and_generate(
    executable: &Path,
    model_dir: &Path,
    request: &ImageGenRequest,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(ImageProgress),
) -> Result<ImageGenResult, String> {
    let argv = command_argv(executable, model_dir);
    let program = argv
        .first()
        .cloned()
        .ok_or_else(|| "image engine command is empty".to_string())?;
    let env = image_child_env_from(std::env::vars(), model_dir);
    let mut cmd = Command::new(&program);
    cmd.args(&argv[1..])
        .env_clear()
        .envs(&env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0000_0200);
    }
    let mut child = cmd.spawn().map_err(|e| {
        format!(
            "cannot start the image engine {}: {e}",
            executable.display()
        )
    })?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "image engine stdin was not piped".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "image engine stdout was not piped".to_string())?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let incoming = read_incoming(&mut reader);
            let stop = matches!(incoming, Ok(Incoming::Eof));
            if tx.send(incoming).is_err() || stop {
                break;
            }
        }
    });
    let result = run_protocol(
        &mut || match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(frame) => frame,
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(Incoming::Idle),
            Err(mpsc::RecvTimeoutError::Disconnected) => Ok(Incoming::Eof),
        },
        &mut |keyword, body| write_line(&mut stdin, keyword, body),
        request,
        cancel,
        on_progress,
    );
    drop(stdin);
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    } else {
        finish_child(&mut child);
    }
    result
}

fn finish_child(child: &mut std::process::Child) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(_) => return,
        }
    }
}

fn write_line(writer: &mut impl Write, keyword: &str, body: &Value) -> Result<(), String> {
    let line = format!(
        "{keyword} {}\n",
        serde_json::to_string(body).map_err(|e| e.to_string())?
    );
    writer
        .write_all(line.as_bytes())
        .map_err(|e| format!("cannot send {keyword} to the image engine ({e})"))?;
    writer
        .flush()
        .map_err(|e| format!("cannot send {keyword} to the image engine ({e})"))?;
    Ok(())
}

#[derive(Debug)]
enum Incoming {
    Idle,
    Eof,
    Ready,
    Progress {
        stage: String,
        step: u32,
        steps: u32,
    },
    Preview,
    Image {
        width: u32,
        height: u32,
        channels: u32,
        seed: Option<u64>,
        steps: Option<u32>,
        rgba: Vec<u8>,
    },
    Error {
        message: String,
    },
    Ignore,
}

fn run_protocol(
    next: &mut dyn FnMut() -> Result<Incoming, String>,
    send_line: &mut dyn FnMut(&str, &Value) -> Result<(), String>,
    request: &ImageGenRequest,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(ImageProgress),
) -> Result<ImageGenResult, String> {
    let prepared = prepare_request(request)?;
    on_progress(ImageProgress::Queued);
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".to_string());
        }
        match next()? {
            Incoming::Idle | Incoming::Ignore => {}
            Incoming::Ready => break,
            Incoming::Error { message } => {
                return Err(format!("the image engine failed to load: {message}"));
            }
            Incoming::Eof => {
                return Err("the image engine exited while loading".to_string());
            }
            Incoming::Progress { .. } | Incoming::Preview | Incoming::Image { .. } => {}
        }
    }
    send_line(
        "GEN",
        &json!({
            "id": REQUEST_ID,
            "prompt": prepared.prompt,
            "width": prepared.width,
            "height": prepared.height,
            "steps": prepared.steps,
            "seed": prepared.seed,
            "preview": 0,
        }),
    )?;
    let mut cancel_sent = false;
    loop {
        if cancel.load(Ordering::Relaxed) && !cancel_sent {
            send_line("CANCEL", &json!({ "id": REQUEST_ID }))?;
            cancel_sent = true;
        }
        match next()? {
            Incoming::Idle | Incoming::Ignore | Incoming::Ready => {}
            Incoming::Progress { stage, step, steps } => {
                on_progress(progress_from_stage(&stage, step, steps));
            }
            Incoming::Preview => {}
            Incoming::Image {
                width,
                height,
                channels,
                seed,
                steps,
                rgba,
            } => {
                if channels != 4 || rgba.len() != width as usize * height as usize * 4 {
                    return Err(format!(
                        "IMAGE frame does not describe its payload ({width}x{height}x{channels}, {} bytes)",
                        rgba.len()
                    ));
                }
                let png = encode_rgba_png(width, height, &rgba)?;
                return Ok(ImageGenResult {
                    width,
                    height,
                    steps: steps.unwrap_or(prepared.steps),
                    seed: seed.unwrap_or(u64::from(prepared.seed)),
                    png,
                });
            }
            Incoming::Error { message } => {
                if message == "cancelled" {
                    return Err("cancelled".to_string());
                }
                return Err(message);
            }
            Incoming::Eof => {
                return Err("the image engine exited before the image".to_string());
            }
        }
    }
}

struct Prepared {
    prompt: String,
    width: u32,
    height: u32,
    steps: u32,
    seed: u32,
}

fn prepare_request(request: &ImageGenRequest) -> Result<Prepared, String> {
    if request.prompt.trim().is_empty() {
        return Err("Prompt is empty.".to_string());
    }
    if request.prompt.len() > MAX_PROMPT_BYTES {
        return Err(format!("Prompt is longer than {MAX_PROMPT_BYTES} bytes."));
    }
    if !valid_side(request.width) || !valid_side(request.height) {
        return Err("Width and height must be multiples of 32, from 256 to 2048.".to_string());
    }
    let seed = match request.seed {
        Some(seed) => seed,
        None => random_seed(),
    };
    Ok(Prepared {
        prompt: request.prompt.clone(),
        width: request.width,
        height: request.height,
        steps: clamp_steps(request.steps),
        seed,
    })
}

fn random_seed() -> u32 {
    if let Ok(mut file) = std::fs::File::open("/dev/urandom") {
        let mut buf = [0u8; 4];
        if file.read_exact(&mut buf).is_ok() {
            return u32::from_ne_bytes(buf) % MAX_RANDOM_SEED;
        }
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(1);
    nanos % MAX_RANDOM_SEED
}

fn progress_from_stage(stage: &str, step: u32, steps: u32) -> ImageProgress {
    match stage {
        "encode" => ImageProgress::Encode,
        "denoise" => ImageProgress::Denoise { step, steps },
        "decode" => ImageProgress::Decode,
        _ => ImageProgress::Working,
    }
}

fn read_incoming(reader: &mut impl BufRead) -> Result<Incoming, String> {
    let Some(line) = read_line_bytes(reader)? else {
        return Ok(Incoming::Eof);
    };
    let text = String::from_utf8_lossy(&line);
    let Some((keyword, body)) = parse_frame_line(&text) else {
        return Ok(Incoming::Ignore);
    };
    if !frame_for_request(&body) && keyword != "READY" && keyword != "ERROR" {
        if keyword == "IMAGE" || keyword == "PREVIEW" {
            let _ = read_payload(reader, &body, keyword)?;
        }
        return Ok(Incoming::Ignore);
    }
    match keyword {
        "READY" => Ok(Incoming::Ready),
        "PROGRESS" => Ok(Incoming::Progress {
            stage: body
                .get("stage")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            step: json_u32(body.get("step")).unwrap_or(0),
            steps: json_u32(body.get("steps")).unwrap_or(0),
        }),
        "PREVIEW" => {
            let _ = read_payload(reader, &body, "PREVIEW")?;
            Ok(Incoming::Preview)
        }
        "IMAGE" => {
            let rgba = read_payload(reader, &body, "IMAGE")?;
            Ok(Incoming::Image {
                width: json_u32(body.get("width")).unwrap_or(0),
                height: json_u32(body.get("height")).unwrap_or(0),
                channels: json_u32(body.get("channels")).unwrap_or(4),
                seed: json_u64(body.get("seed")),
                steps: json_u32(body.get("steps")),
                rgba,
            })
        }
        "ERROR" => Ok(Incoming::Error {
            message: body
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown engine error")
                .to_string(),
        }),
        _ => Ok(Incoming::Ignore),
    }
}

fn frame_for_request(body: &Value) -> bool {
    match body.get("id").and_then(|v| v.as_str()) {
        None | Some("") | Some(REQUEST_ID) => true,
        Some(_) => false,
    }
}

fn parse_frame_line(line: &str) -> Option<(&str, Value)> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let (keyword, rest) = line.split_once(' ')?;
    if !matches!(
        keyword,
        "READY" | "PROGRESS" | "PREVIEW" | "IMAGE" | "ERROR"
    ) {
        return None;
    }
    let body: Value = serde_json::from_str(rest).ok()?;
    if !body.is_object() {
        return None;
    }
    Some((keyword, body))
}

fn read_payload(reader: &mut impl BufRead, body: &Value, keyword: &str) -> Result<Vec<u8>, String> {
    let Some(size) = json_u64(body.get("bytes")) else {
        return Err(format!("invalid {keyword} frame size"));
    };
    if size > MAX_FRAME_BYTES {
        return Err(format!("invalid {keyword} frame size {size}"));
    }
    let mut payload = vec![0u8; size as usize];
    reader
        .read_exact(&mut payload)
        .map_err(|_| "the image engine closed its output in the middle of a frame".to_string())?;
    let mut nl = [0u8; 1];
    reader
        .read_exact(&mut nl)
        .map_err(|_| format!("{keyword} payload is not followed by a newline"))?;
    if nl[0] != b'\n' {
        return Err(format!("{keyword} payload is not followed by a newline"));
    }
    Ok(payload)
}

fn read_line_bytes(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>, String> {
    let mut buf = Vec::new();
    let n = reader
        .read_until(b'\n', &mut buf)
        .map_err(|e| format!("image engine read failed ({e})"))?;
    if n == 0 {
        return Ok(None);
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
    }
    if buf.last() == Some(&b'\r') {
        buf.pop();
    }
    Ok(Some(buf))
}

fn json_u32(value: Option<&Value>) -> Option<u32> {
    let n = json_u64(value)?;
    u32::try_from(n).ok()
}

fn json_u64(value: Option<&Value>) -> Option<u64> {
    let value = value?;
    if value.is_boolean() {
        return None;
    }
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|n| u64::try_from(n).ok()))
}

/// Encode RGBA8 as a PNG the GPUI image decoder can read.
///
/// Stored zlib blocks only (filter 0, no compressor crate).
pub fn encode_rgba_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 {
        return Err("image size is empty".to_string());
    }
    let row = width as usize * 4;
    let need = row
        .checked_mul(height as usize)
        .ok_or_else(|| "image is too large".to_string())?;
    if rgba.len() != need {
        return Err(format!(
            "RGBA length {} does not match {width}x{height}",
            rgba.len()
        ));
    }
    let mut raw = Vec::with_capacity(need + height as usize);
    for y in 0..height as usize {
        raw.push(0);
        let start = y * row;
        raw.extend_from_slice(&rgba[start..start + row]);
    }
    let z = zlib_stored(&raw, 65535);
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut png = Vec::new();
    png.extend_from_slice(&[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n']);
    png.extend(png_chunk(b"IHDR", &ihdr));
    png.extend(png_chunk(b"IDAT", &z));
    png.extend(png_chunk(b"IEND", &[]));
    Ok(png)
}

fn zlib_stored(data: &[u8], chunk: usize) -> Vec<u8> {
    let chunk = chunk.clamp(1, 65535);
    let mut out = vec![0x78, 0x01];
    if data.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xff, 0xff]);
    } else {
        let mut offset = 0;
        while offset < data.len() {
            let end = (offset + chunk).min(data.len());
            let last = end == data.len();
            let piece = &data[offset..end];
            let len = piece.len() as u16;
            out.push(if last { 0x01 } else { 0x00 });
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&(!len).to_le_bytes());
            out.extend_from_slice(piece);
            offset = end;
        }
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + data.len());
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_src = Vec::with_capacity(4 + data.len());
    crc_src.extend_from_slice(kind);
    crc_src.extend_from_slice(data);
    out.extend_from_slice(&png_crc(&crc_src).to_be_bytes());
    out
}

fn png_crc(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn sample_request() -> ImageGenRequest {
        ImageGenRequest {
            prompt: "a fox".into(),
            width: 256,
            height: 256,
            steps: 8,
            seed: Some(7),
        }
    }

    fn drive(
        script: &[u8],
        request: &ImageGenRequest,
        cancel: &AtomicBool,
    ) -> (Result<ImageGenResult, String>, String) {
        let mut reader = Cursor::new(script.to_vec());
        let mut written = Vec::new();
        let result = run_protocol(
            &mut || read_incoming(&mut reader),
            &mut |keyword, body| {
                let line = format!("{keyword} {body}\n");
                written.extend_from_slice(line.as_bytes());
                Ok(())
            },
            request,
            cancel,
            &mut |_| {},
        );
        (result, String::from_utf8_lossy(&written).into_owned())
    }

    #[test]
    fn defaults_and_side_rules() {
        assert_eq!(default_image_size(), (768, 512));
        assert_eq!(default_steps(), 8);
        assert_eq!(size_presets().len(), 6);
        assert!(
            size_presets()
                .iter()
                .all(|(w, h)| valid_side(*w) && valid_side(*h))
        );
        assert!(valid_side(256) && valid_side(2048) && valid_side(768));
        assert!(!valid_side(255) && !valid_side(2080) && !valid_side(1000) && !valid_side(0));
        assert_eq!(clamp_steps(1), 2);
        assert_eq!(clamp_steps(0), 2);
        assert_eq!(clamp_steps(8), 8);
        assert_eq!(clamp_steps(200), 200);
        assert_eq!(clamp_steps(201), 200);
    }

    #[test]
    fn png_matches_stored_pixels_and_known_crc() {
        assert_eq!(png_crc(b"123456789"), 0xcbf4_3926);
        assert_eq!(adler32(b""), 1);
        let png = encode_rgba_png(1, 1, &[10, 20, 30, 40]).unwrap();
        assert_eq!(
            &png[..8],
            &[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n']
        );
        let raw = inflate_stored_idat(&png).unwrap();
        assert_eq!(raw, vec![0, 10, 20, 30, 40]);
        let wide = vec![7u8; 32];
        let z = zlib_stored(&wide, 8);
        assert!(z.windows(2).any(|w| w == [0x00, 0x08]) || z[2] == 0x00);
        assert_eq!(inflate_zlib_stored(&z).unwrap(), wide);
    }

    #[test]
    fn protocol_stray_line_then_image() {
        let rgba = [1u8, 2, 3, 4].repeat(256 * 256);
        let mut script = b"not a frame\nREADY {\"ok\":true}\n".to_vec();
        let header = format!(
            "IMAGE {{\"id\":\"r1\",\"width\":256,\"height\":256,\"channels\":4,\"bytes\":{},\"seed\":9,\"steps\":8}}\n",
            rgba.len()
        );
        script.extend_from_slice(header.as_bytes());
        script.extend_from_slice(&rgba);
        script.push(b'\n');
        let cancel = AtomicBool::new(false);
        let (result, written) = drive(&script, &sample_request(), &cancel);
        let result = result.expect("image");
        assert!(written.contains("\"preview\":0"), "{written}");
        assert!(written.contains("\"id\":\"r1\""), "{written}");
        assert_eq!(result.width, 256);
        assert_eq!(result.height, 256);
        assert_eq!(result.steps, 8);
        assert_eq!(result.seed, 9);
        assert!(result.png.starts_with(&[0x89, b'P', b'N', b'G']));
    }

    #[test]
    fn protocol_cancelled_error() {
        let script = b"READY {\"ok\":true}\nERROR {\"id\":\"r1\",\"message\":\"cancelled\"}\n";
        let mut reader = Cursor::new(script.to_vec());
        let mut written = Vec::new();
        let cancel = AtomicBool::new(false);
        let result = run_protocol(
            &mut || read_incoming(&mut reader),
            &mut |keyword, body| {
                if keyword == "GEN" {
                    cancel.store(true, Ordering::Relaxed);
                }
                written.extend_from_slice(format!("{keyword} {body}\n").as_bytes());
                Ok(())
            },
            &sample_request(),
            &cancel,
            &mut |_| {},
        );
        let err = result.expect_err("cancel");
        assert!(err.contains("cancelled"), "{err}");
        let written = String::from_utf8_lossy(&written);
        assert!(written.contains("CANCEL"), "{written}");
    }

    #[test]
    fn protocol_rejects_bad_channels() {
        let script = b"READY {\"ok\":true}\nIMAGE {\"id\":\"r1\",\"width\":256,\"height\":256,\"channels\":3,\"bytes\":3,\"seed\":1,\"steps\":8}\nxyz\n";
        let cancel = AtomicBool::new(false);
        let (result, _) = drive(script, &sample_request(), &cancel);
        let err = result.expect_err("channels");
        assert!(err.contains("channels") || err.contains("payload"), "{err}");
    }

    #[test]
    fn child_env_omits_xdna_and_argv_prefixes_python() {
        let env = image_child_env_from(
            [
                ("COLI_XDNA".into(), "1".into()),
                ("PATH".into(), "/bin".into()),
            ],
            Path::new("/models/qwen"),
        );
        assert!(!env.contains_key("COLI_XDNA"));
        assert_eq!(env.get("SERVE").map(String::as_str), Some("1"));
        assert_eq!(env.get("SNAP").map(String::as_str), Some("/models/qwen"));
        let argv = command_argv(Path::new("tools/qwenimage_stub.py"), Path::new("/m"));
        assert_eq!(argv[0], "python3");
        assert!(argv.iter().any(|a| a == "--serve"));
        let bin = command_argv(Path::new("/usr/libexec/colibri/qwenimage"), Path::new("/m"));
        assert_eq!(bin[0], "/usr/libexec/colibri/qwenimage");
        assert_ne!(bin[0], "python3");
    }

    #[test]
    fn candidates_do_not_consult_an_engine_override_slot() {
        let cwd = Path::new("/work");
        let exe = Path::new("/opt/colibri");
        let cands = qwenimage_candidates(Some(cwd), Some(exe));
        assert_eq!(cands[0], cwd.join("c").join("qwenimage"));
        assert_eq!(cands[1], cwd.join("qwenimage"));
        assert!(
            cands
                .iter()
                .any(|p| p.ends_with("libexec/colibri/qwenimage"))
        );
        assert!(
            cands
                .iter()
                .all(|p| !p.to_string_lossy().contains("COLI_ENGINE"))
        );
    }

    fn inflate_stored_idat(png: &[u8]) -> Result<Vec<u8>, String> {
        let mut i = 8;
        while i + 8 <= png.len() {
            let len = u32::from_be_bytes(png[i..i + 4].try_into().unwrap()) as usize;
            let kind = &png[i + 4..i + 8];
            let data = &png[i + 8..i + 8 + len];
            if kind == b"IDAT" {
                return inflate_zlib_stored(data);
            }
            i += 12 + len;
        }
        Err("no IDAT".into())
    }

    fn inflate_zlib_stored(z: &[u8]) -> Result<Vec<u8>, String> {
        if z.len() < 6 || z[0] != 0x78 {
            return Err("not a zlib header".into());
        }
        let mut i = 2;
        let mut out = Vec::new();
        loop {
            if i >= z.len() {
                return Err("truncated zlib".into());
            }
            let header = z[i];
            i += 1;
            if i + 4 > z.len() {
                return Err("truncated stored header".into());
            }
            let len = u16::from_le_bytes([z[i], z[i + 1]]) as usize;
            i += 4;
            if i + len > z.len() {
                return Err("truncated stored body".into());
            }
            out.extend_from_slice(&z[i..i + len]);
            i += len;
            if header & 1 == 1 {
                break;
            }
        }
        Ok(out)
    }
}
