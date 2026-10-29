//! The XMem worker protocol (JSON lines over stdio) and the frame exporter that
//! prepares a worker's input cache.
//!
//! R2 proves the transport and the Rust-side preprocessing without PyTorch,
//! XMem or weights. The worker is a separate process: the editor never links
//! PyTorch and never needs a system Python. A real worker is started only when
//! the optional pack and consented weights exist (R3/R5).

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use filmcraft_media::{FrameRequest, MediaSource};
use filmcraft_time::{FrameRate, TimeRange};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The protocol version both sides must speak.
pub const WORKER_PROTOCOL: u32 = 1;

/// One JSON line is capped so a broken or hostile worker cannot make the editor
/// allocate without bound.
const MAX_WORKER_LINE: usize = 1_048_576;

/// The tail of stderr kept for error reports.
const STDERR_TAIL: usize = 16_384;

/// A frame export larger than this is almost certainly a bad range, not a clip.
const MAX_EXPORT_FRAMES: i64 = 100_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerDirection {
    Forward,
    Backward,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum WorkerCommand {
    Hello {
        protocol: u32,
    },
    Propagate {
        frames_dir: String,
        first_frame: i64,
        last_frame: i64,
        initial_mask: String,
        output_dir: String,
        width: u32,
        height: u32,
        model: String,
        model_sha256: String,
        direction: WorkerDirection,
    },
    Cancel,
    Shutdown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum WorkerEvent {
    Ready { protocol: u32, backend: String, torch: String, xmem: String },
    Progress { frame: i64, total: i64, elapsed: f64, fps: f64 },
    Done { frames: i64, elapsed: f64 },
    Error { message: String },
    Cancelled,
}

/// What a successful `hello` handshake reported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkerInfo {
    pub protocol: u32,
    pub backend: String,
    pub torch: String,
    pub xmem: String,
}

#[derive(Debug, Error)]
pub enum WorkerError {
    #[error("roto worker I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("roto worker protocol: {0}")]
    Protocol(String),
    #[error("roto worker closed unexpectedly: {0}")]
    Closed(String),
    #[error("roto worker timed out: {0}")]
    Timeout(String),
    #[error("roto worker JSON: {0}")]
    Json(#[from] serde_json::Error),
}

/// A child process speaking the JSON-lines protocol. A reader thread turns
/// stdout lines into channel events so a caller can apply a deadline instead
/// of blocking forever on a stuck worker.
pub struct XmemWorker {
    child: Child,
    stdin: ChildStdin,
    events: Receiver<Result<WorkerEvent, WorkerError>>,
    reader_thread: Option<JoinHandle<()>>,
    stderr: Arc<Mutex<String>>,
    stderr_thread: Option<JoinHandle<()>>,
}

impl XmemWorker {
    /// Start `program` and speak to its stdin/stdout. `args` is the worker
    /// runtime's own command line (for example `-u <script>` for Python).
    pub fn spawn(program: &Path, args: &[&str]) -> Result<Self, WorkerError> {
        let mut command = Command::new(program);
        command.args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // A GUI editor must not flash a console window for its worker.
            command.creation_flags(0x0800_0000);
        }
        let mut child = command.spawn()?;
        let stdin = child.stdin.take().ok_or_else(|| WorkerError::Protocol("worker stdin is not piped".to_string()))?;
        let stdout = child.stdout.take().ok_or_else(|| WorkerError::Protocol("worker stdout is not piped".to_string()))?;
        let stderr = child.stderr.take().ok_or_else(|| WorkerError::Protocol("worker stderr is not piped".to_string()))?;

        let tail = Arc::new(Mutex::new(String::new()));
        let tail_for_thread = Arc::clone(&tail);
        let stderr_thread = std::thread::Builder::new().name("xmem-worker-stderr".to_string()).spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut chunk = [0u8; 4096];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        let text = String::from_utf8_lossy(&chunk[..read]);
                        let mut buffer = tail_for_thread.lock().unwrap_or_else(PoisonError::into_inner);
                        buffer.push_str(&text);
                        if buffer.len() > STDERR_TAIL {
                            let mut cut = buffer.len() - STDERR_TAIL;
                            while cut < buffer.len() && !buffer.is_char_boundary(cut) {
                                cut += 1;
                            }
                            buffer.drain(..cut);
                        }
                    }
                }
            }
        })?;

        let (tx, events) = mpsc::channel();
        let reader_thread = std::thread::Builder::new().name("xmem-worker-stdout".to_string()).spawn(move || read_events(stdout, tx))?;

        Ok(Self { child, stdin, events, reader_thread: Some(reader_thread), stderr: tail, stderr_thread: Some(stderr_thread) })
    }

    /// Send `hello`, then require a matching `ready` event.
    pub fn handshake(&mut self) -> Result<WorkerInfo, WorkerError> {
        self.send(&WorkerCommand::Hello { protocol: WORKER_PROTOCOL })?;
        let event = self.recv()?;
        Self::expect_ready(event)
    }

    /// A handshake with a deadline, so a missing or hung runtime cannot freeze
    /// the app. On timeout the caller drops (or kills) the worker.
    pub fn handshake_timeout(&mut self, timeout: Duration) -> Result<WorkerInfo, WorkerError> {
        self.send(&WorkerCommand::Hello { protocol: WORKER_PROTOCOL })?;
        let event = self.recv_timeout(timeout)?;
        Self::expect_ready(event)
    }

    fn expect_ready(event: WorkerEvent) -> Result<WorkerInfo, WorkerError> {
        match event {
            WorkerEvent::Ready { protocol, backend, torch, xmem } => {
                if protocol != WORKER_PROTOCOL {
                    return Err(WorkerError::Protocol(format!("worker speaks protocol {protocol}, expected {WORKER_PROTOCOL}")));
                }
                Ok(WorkerInfo { protocol, backend, torch, xmem })
            }
            WorkerEvent::Error { message } => Err(WorkerError::Protocol(format!("worker error: {message}"))),
            other => Err(WorkerError::Protocol(format!("expected ready, got {other:?}"))),
        }
    }

    pub fn send(&mut self, command: &WorkerCommand) -> Result<(), WorkerError> {
        let json = serde_json::to_string(command)?;
        self.stdin.write_all(json.as_bytes())?;
        self.stdin.write_all(b"\n")?;
        self.stdin.flush()?;
        Ok(())
    }

    /// Read one event. A worker that closes the pipe is an error with the
    /// captured stderr tail, never a silent end of job.
    pub fn recv(&mut self) -> Result<WorkerEvent, WorkerError> {
        match self.events.recv() {
            Ok(Ok(event)) => Ok(event),
            Ok(Err(WorkerError::Closed(_))) => Err(WorkerError::Closed(self.stderr_tail())),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(WorkerError::Closed(self.stderr_tail())),
        }
    }

    /// Read one event, giving up after `timeout`. A quiet worker is a timeout,
    /// not an error: the caller decides whether to cancel or kill.
    pub fn recv_timeout(&mut self, timeout: Duration) -> Result<WorkerEvent, WorkerError> {
        match self.events.recv_timeout(timeout) {
            Ok(Ok(event)) => Ok(event),
            Ok(Err(WorkerError::Closed(_))) => Err(WorkerError::Closed(self.stderr_tail())),
            Ok(Err(error)) => Err(error),
            Err(RecvTimeoutError::Timeout) => Err(WorkerError::Timeout(format!("no event within {:.1}s", timeout.as_secs_f64()))),
            Err(RecvTimeoutError::Disconnected) => Err(WorkerError::Closed(self.stderr_tail())),
        }
    }

    /// Terminate the child immediately. `Drop` also kills, but a job runner
    /// calls this when a deadline or memory ceiling is hit so the failure is
    /// explicit in its log.
    pub fn kill(&mut self) -> Result<(), WorkerError> {
        self.child.kill()?;
        Ok(())
    }

    pub fn request_shutdown(&mut self) -> Result<(), WorkerError> {
        self.send(&WorkerCommand::Shutdown)
    }

    /// The captured stderr tail, for a status bar or a bug report.
    pub fn stderr_tail(&self) -> String {
        let text = self.stderr.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let text = text.trim();
        if text.is_empty() { "no stderr output".to_string() } else { text.to_string() }
    }
}

impl Drop for XmemWorker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(thread) = self.reader_thread.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.stderr_thread.take() {
            let _ = thread.join();
        }
    }
}

/// Parse stdout lines off the blocking pipe so the caller can use a deadline.
fn read_events(stdout: ChildStdout, tx: mpsc::Sender<Result<WorkerEvent, WorkerError>>) {
    let mut reader = BufReader::new(stdout);
    loop {
        let mut line = String::new();
        match (&mut reader).take(MAX_WORKER_LINE as u64).read_line(&mut line) {
            Ok(0) => {
                let _ = tx.send(Err(WorkerError::Closed("worker stdout closed".to_string())));
                break;
            }
            Ok(read) => {
                if read >= MAX_WORKER_LINE && !line.ends_with('\n') {
                    let _ = tx.send(Err(WorkerError::Protocol(format!("worker line exceeds {MAX_WORKER_LINE} bytes"))));
                    break;
                }
                let line = line.trim();
                if line.is_empty() {
                    let _ = tx.send(Err(WorkerError::Protocol("worker sent an empty line".to_string())));
                    break;
                }
                match serde_json::from_str(line) {
                    Ok(event) => {
                        if tx.send(Ok(event)).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        let _ = tx.send(Err(WorkerError::Protocol(format!("invalid worker event: {error}: {line}"))));
                        break;
                    }
                }
            }
            Err(error) => {
                let _ = tx.send(Err(WorkerError::Io(error)));
                break;
            }
        }
    }
}

#[derive(Debug, Error)]
pub enum FrameExportError {
    #[error("roto frame export needs a non-empty range")]
    EmptyRange,
    #[error("roto frame export: {0}")]
    Invalid(String),
    #[error("roto frame export I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("roto frame export image: {0}")]
    Image(#[from] image::ImageError),
    #[error("roto frame export media: {0}")]
    Media(#[from] filmcraft_media::MediaError),
}

/// The frames written for a worker run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportedFrames {
    pub dir: PathBuf,
    pub first_frame: i64,
    pub last_frame: i64,
    pub width: u32,
    pub height: u32,
    pub count: usize,
}

/// Decode `source` over `range` through FilmCraft's codecs, scale to fit
/// `max_size` (never upscaling), and write `frame_<index>.png` in media order.
pub fn export_frames(
    source: &dyn MediaSource,
    range: TimeRange,
    rate: FrameRate,
    output_dir: &Path,
    max_size: (u32, u32),
) -> Result<ExportedFrames, FrameExportError> {
    if range.duration.0 <= 0 {
        return Err(FrameExportError::EmptyRange);
    }
    if rate.num <= 0 || rate.den <= 0 {
        return Err(FrameExportError::Invalid(format!("invalid frame rate {}/{}", rate.num, rate.den)));
    }
    if max_size.0 == 0 || max_size.1 == 0 {
        return Err(FrameExportError::Invalid(format!("invalid working size {}x{}", max_size.0, max_size.1)));
    }
    let end_tick = range.start.0.checked_add(range.duration.0).ok_or_else(|| FrameExportError::Invalid("frame range end overflows".to_string()))?;
    let first_frame = rate.frame_at(range.start);
    let last_exclusive = rate.frame_at(filmcraft_time::Tick(end_tick));
    if first_frame < 0 {
        return Err(FrameExportError::Invalid(format!("negative first frame {first_frame}")));
    }
    let count = last_exclusive.checked_sub(first_frame).ok_or_else(|| FrameExportError::Invalid("frame count overflows".to_string()))?;
    if count <= 0 {
        return Err(FrameExportError::EmptyRange);
    }
    if count > MAX_EXPORT_FRAMES {
        return Err(FrameExportError::Invalid(format!("{count} frames exceeds the {MAX_EXPORT_FRAMES}-frame export limit")));
    }

    let (source_width, source_height) = {
        let video = source.info().video.as_ref().ok_or_else(|| FrameExportError::Invalid("source has no video stream".to_string()))?;
        (video.width, video.height)
    };
    let (width, height) = fit_working_size(source_width, source_height, max_size)?;
    std::fs::create_dir_all(output_dir)?;

    for offset in 0..count {
        let frame = first_frame.checked_add(offset).ok_or_else(|| FrameExportError::Invalid("frame index overflows".to_string()))?;
        let offset_ticks = rate.tick_of(offset);
        let tick =
            filmcraft_time::Tick(range.start.0.checked_add(offset_ticks.0).ok_or_else(|| FrameExportError::Invalid("frame time overflows".to_string()))?);
        let decoded = source.video_frame(FrameRequest::full(tick))?;
        let rgba = decoded.to_rgba8();
        if decoded.width != width || decoded.height != height {
            let image = image::RgbaImage::from_raw(decoded.width, decoded.height, rgba)
                .ok_or_else(|| FrameExportError::Invalid(format!("decoded frame buffer does not match {}x{}", decoded.width, decoded.height)))?;
            let resized = image::imageops::resize(&image, width, height, image::imageops::FilterType::Triangle);
            save_rgba(&resized, output_dir, frame)?;
        } else {
            let image = image::RgbaImage::from_raw(width, height, rgba)
                .ok_or_else(|| FrameExportError::Invalid(format!("decoded frame buffer does not match {width}x{height}")))?;
            save_rgba(&image, output_dir, frame)?;
        }
    }

    Ok(ExportedFrames {
        dir: output_dir.to_path_buf(),
        first_frame,
        last_frame: first_frame.checked_add(count - 1).ok_or_else(|| FrameExportError::Invalid("last frame overflows".to_string()))?,
        width,
        height,
        count: usize::try_from(count).map_err(|_| FrameExportError::Invalid("frame count does not fit usize".to_string()))?,
    })
}

fn save_rgba(image: &image::RgbaImage, output_dir: &Path, frame: i64) -> Result<(), FrameExportError> {
    image.save(output_dir.join(format!("frame_{frame:08}.png")))?;
    Ok(())
}

fn fit_working_size(width: u32, height: u32, max_size: (u32, u32)) -> Result<(u32, u32), FrameExportError> {
    if width == 0 || height == 0 {
        return Err(FrameExportError::Invalid("source video size is zero".to_string()));
    }
    let scale_x = f64::from(max_size.0) / f64::from(width);
    let scale_y = f64::from(max_size.1) / f64::from(height);
    let scale = scale_x.min(scale_y).min(1.0);
    if !scale.is_finite() || scale <= 0.0 {
        return Err(FrameExportError::Invalid("working size scale is not finite".to_string()));
    }
    let out_width = ((f64::from(width) * scale).round() as u32).max(1);
    let out_height = ((f64::from(height) * scale).round() as u32).max(1);
    Ok((out_width, out_height))
}
