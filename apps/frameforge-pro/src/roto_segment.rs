//! Running a segmentation plan: write the request, spawn the worker, check the
//! reply, adopt the mask.
//!
//! The plan is built and validated in `frameforge-roto`, which knows the
//! protocol; this module is the part that touches the disk and the operating
//! system, and it is deliberately narrow:
//!
//! * a **stale reply is deleted before the worker starts**, so an old file can
//!   never be read as this run's answer;
//! * the worker's exit status, its stderr tail and the reply are all checked —
//!   a nonzero exit is a failure even if a reply file exists;
//! * the mask is loaded at the size the request named, and a different size is
//!   an error rather than something to resize;
//! * the whole thing runs on a worker thread with a timeout and a cancel flag,
//!   so the UI stays responsive and a hung Python process cannot freeze the app.
//!
//! No test here starts a real model: the failure paths are exercised, the reply
//! and mask handling use files the test writes itself.

#![allow(dead_code)] // The run surface is complete; the panel reports what it uses.

use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use frameforge_roto::segment::{SegmentPlan, SegmentReply, SegmentRequest};

/// How long a segmentation worker gets before it is killed.
pub const SEGMENT_TIMEOUT: Duration = Duration::from_secs(600);

/// A finished selection: the mask as 8-bit coverage at the request's size.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SegmentOutcome {
    pub coverage: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub summary: String,
}

/// A segmentation running on its own thread.
pub struct SegmentRun {
    cancel: Arc<AtomicBool>,
    done: Arc<Mutex<Option<Result<SegmentOutcome, String>>>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl SegmentRun {
    /// Start the worker thread. The plan must already be non-dormant: the caller
    /// is the user's click.
    pub fn spawn(plan: SegmentPlan, timeout: Duration) -> Result<Self, String> {
        if plan.dormant {
            return Err("this plan is dormant; a run needs the user's action".to_string());
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let done: Arc<Mutex<Option<Result<SegmentOutcome, String>>>> = Arc::new(Mutex::new(None));
        let thread_cancel = Arc::clone(&cancel);
        let thread_done = Arc::clone(&done);
        let handle = std::thread::Builder::new()
            .name("frameforge-roto-segment".to_string())
            .spawn(move || {
                let result = execute(&plan, timeout, &thread_cancel);
                if let Ok(mut slot) = thread_done.lock() {
                    *slot = Some(result);
                }
            })
            .map_err(|error| format!("could not start the segmentation thread: {error}"))?;
        Ok(Self { cancel, done, handle: Some(handle) })
    }

    /// The outcome once the thread has finished, exactly once.
    pub fn try_finish(&mut self) -> Option<Result<SegmentOutcome, String>> {
        let finished = self.handle.as_ref().is_some_and(std::thread::JoinHandle::is_finished);
        if !finished {
            return None;
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        self.done.lock().ok().and_then(|mut slot| slot.take())
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_running(&self) -> bool {
        self.handle.as_ref().is_some_and(|handle| !handle.is_finished())
    }
}

/// Write the plan's request JSON where the plan says, atomically.
pub fn write_request(plan: &SegmentPlan) -> Result<(), String> {
    let path = &plan.request_path;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, plan.request_json.as_bytes()).map_err(|error| format!("could not write {}: {error}", temporary.display()))?;
    std::fs::rename(&temporary, path).map_err(|error| format!("could not place {}: {error}", path.display()))
}

/// Read the worker's reply and check it against the request it was given.
///
/// A worker that failed is expected to leave `{"version": 1, "error": "…"}`;
/// that reason is returned verbatim, because it names the missing file or the
/// unimportable package the user has to fix.
pub fn read_reply(plan: &SegmentPlan) -> Result<SegmentReply, String> {
    let text = std::fs::read_to_string(&plan.reply_path).map_err(|error| format!("the worker wrote no reply at {}: {error}", plan.reply_path.display()))?;
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text)
        && let Some(error) = value.get("error").and_then(serde_json::Value::as_str)
    {
        return Err(error.to_string());
    }
    let reply = SegmentReply::parse(&text).map_err(|error| error.to_string())?;
    let request = SegmentRequest::from_json(&plan.request_json).map_err(|error| error.to_string())?;
    reply.validate(&request).map_err(|error| error.to_string())?;
    Ok(reply)
}

/// Load the mask the worker wrote as 8-bit coverage, at exactly the size the
/// request named. A different size is an error: resizing a mask silently would
/// move every edge.
pub fn adopt_mask(reply: &SegmentReply, expected: (u32, u32)) -> Result<Vec<u8>, String> {
    let path = reply.mask_path();
    let image = image::open(&path).map_err(|error| format!("the worker's mask {} could not be read: {error}", path.display()))?;
    let gray = image.to_luma8();
    if gray.dimensions() != expected {
        return Err(format!("the worker's mask is {}x{}, but the request was {}x{}", gray.width(), gray.height(), expected.0, expected.1));
    }
    Ok(gray.into_raw())
}

/// The thread's whole body: write, spawn, wait, read back.
pub fn execute(plan: &SegmentPlan, timeout: Duration, cancel: &AtomicBool) -> Result<SegmentOutcome, String> {
    let request = SegmentRequest::from_json(&plan.request_json).map_err(|error| error.to_string())?;
    write_request(plan)?;
    // A reply from a previous attempt must never be read as this run's answer.
    if plan.reply_path.exists() {
        std::fs::remove_file(&plan.reply_path).map_err(|error| format!("could not clear the stale reply {}: {error}", plan.reply_path.display()))?;
    }

    let mut command = Command::new(&plan.command.program);
    command.args(&plan.command.args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| format!("could not start the worker {}: {error}", plan.command.program))?;

    // Drain stderr on its own thread: a full pipe would otherwise block the
    // worker, which would look like a hang.
    let stderr_sink = Arc::new(Mutex::new(String::new()));
    if let Some(stderr) = child.stderr.take() {
        let sink = Arc::clone(&stderr_sink);
        std::thread::spawn(move || {
            use std::io::Read;
            let mut text = String::new();
            let _ = std::io::BufReader::new(stderr).read_to_string(&mut text);
            if let Ok(mut slot) = sink.lock() {
                *slot = text;
            }
        });
    }

    let started = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("the segmentation was cancelled".to_string());
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    break;
                }
                let reason = stderr_sink.lock().map(|text| text.clone()).unwrap_or_default();
                let tail: String = reason.trim().chars().rev().take(600).collect::<String>().chars().rev().collect();
                return Err(format!("the worker exited with {status}: {tail}"));
            }
            Ok(None) => {
                if started.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("the worker did not finish within {} seconds", timeout.as_secs()));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(error) => return Err(format!("could not check on the worker: {error}")),
        }
    }

    let reply = read_reply(plan)?;
    let coverage = adopt_mask(&reply, (request.width, request.height))?;
    Ok(SegmentOutcome { coverage, width: request.width, height: request.height, summary: reply.summary() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use frameforge_roto::engine::ExternalCommand;
    use frameforge_roto::selection::Prompt;
    use std::path::{Path, PathBuf};

    fn root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("frameforge-segment-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn request_json() -> String {
        SegmentRequest::new(0, 8, 4, vec![Prompt::Point { x: 2.0, y: 2.0, positive: true, frame: 0 }], 480).expect("request").to_json().expect("json")
    }

    fn plan(directory: &Path, program: &str) -> SegmentPlan {
        SegmentPlan {
            protocol: frameforge_roto::SEGMENT_PROTOCOL,
            model: "sam2.1-tiny".to_string(),
            command: ExternalCommand { program: program.to_string(), args: vec!["-u".to_string(), "w.py".to_string()] },
            request_path: directory.join("request.json"),
            reply_path: directory.join("reply.json"),
            request_json: request_json(),
            weights: vec![PathBuf::from("w.pt")],
            dormant: false,
        }
    }

    #[test]
    fn the_request_is_written_exactly_and_atomically() {
        let directory = root("request");
        let plan = plan(&directory, "python.exe");
        write_request(&plan).expect("write");
        let written = std::fs::read_to_string(&plan.request_path).expect("read");
        assert_eq!(written, plan.request_json);
        assert!(!directory.join("request.json.tmp").exists(), "the temporary file is gone");
        assert_eq!(SegmentRequest::from_json(&written).expect("reparse"), SegmentRequest::from_json(&plan.request_json).expect("original"));
    }

    #[test]
    fn an_error_reply_is_reported_verbatim() {
        let directory = root("error");
        let plan = plan(&directory, "python.exe");
        std::fs::write(&plan.reply_path, r#"{"version":1,"error":"the checkpoint does not exist: C:\\m\\no.pt"}"#).expect("reply");
        let error = read_reply(&plan).expect_err("an error reply is a failure");
        assert!(error.contains("the checkpoint does not exist"), "{error}");
    }

    #[test]
    fn a_reply_for_another_frame_or_size_is_refused() {
        let directory = root("mismatch");
        let plan = plan(&directory, "python.exe");
        std::fs::write(&plan.reply_path, r#"{"version":1,"frame":3,"width":8,"height":4,"mask":"m.png"}"#).expect("reply");
        assert!(read_reply(&plan).expect_err("wrong frame").contains("frame"));
        std::fs::write(&plan.reply_path, r#"{"version":1,"frame":0,"width":9,"height":4,"mask":"m.png"}"#).expect("reply");
        assert!(read_reply(&plan).expect_err("wrong size").contains("9x4"));
    }

    #[test]
    fn a_reply_that_is_not_there_says_so() {
        let directory = root("absent");
        let plan = plan(&directory, "python.exe");
        let error = read_reply(&plan).expect_err("no reply");
        assert!(error.contains("wrote no reply"), "{error}");
    }

    #[test]
    fn the_mask_must_match_the_requested_size_and_is_adopted_otherwise() {
        let directory = root("mask");
        let reply = SegmentReply {
            version: 1,
            frame: 0,
            width: 8,
            height: 4,
            mask: directory.join("mask.png").display().to_string(),
            score: Some(0.9),
            provider: Some("cpu".to_string()),
            elapsed_ms: Some(12),
        };
        let mut image = image::GrayImage::new(8, 4);
        image.put_pixel(2, 2, image::Luma([255]));
        image.save(&reply.mask).expect("save mask");
        let coverage = adopt_mask(&reply, (8, 4)).expect("adopt");
        assert_eq!(coverage.len(), 32);
        assert_eq!(coverage[2 * 8 + 2], 255);
        assert_eq!(coverage[0], 0);

        let mut wrong = image::GrayImage::new(4, 4);
        wrong.put_pixel(0, 0, image::Luma([255]));
        wrong.save(&reply.mask).expect("save mask");
        let error = adopt_mask(&reply, (8, 4)).expect_err("wrong size");
        assert!(error.contains("4x4"), "{error}");
    }

    #[test]
    fn a_worker_that_cannot_start_names_the_program_rather_than_hanging() {
        let directory = root("nospawn");
        let plan = plan(&directory, "this-program-does-not-exist-9f2a");
        let cancel = AtomicBool::new(false);
        let error = execute(&plan, Duration::from_secs(5), &cancel).expect_err("no such program");
        assert!(error.contains("this-program-does-not-exist-9f2a"), "{error}");
        assert!(plan.request_path.is_file(), "the request was still written, so a rerun is reproducible");
    }

    #[test]
    fn a_dormant_plan_never_starts_a_thread() {
        let directory = root("dormant");
        let mut plan = plan(&directory, "python.exe");
        plan.dormant = true;
        assert!(SegmentRun::spawn(plan, Duration::from_secs(5)).is_err());
    }
}
