//! The XMem worker protocol and frame exporter (R2). Stubs only: the protocol,
//! process client and PNG exporter land after the acceptance tests are red.

use std::path::Path;

use filmcraft_media::MediaSource;
use filmcraft_time::{FrameRate, TimeRange};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const WORKER_PROTOCOL: u32 = 1;

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkerInfo {
    pub protocol: u32,
    pub backend: String,
    pub torch: String,
    pub xmem: String,
}

#[derive(Debug, Error)]
pub enum WorkerError {
    #[error("roto worker is not available: {0}")]
    Unavailable(String),
}

pub struct XmemWorker;

impl XmemWorker {
    pub fn spawn(_program: &Path, _args: &[&str]) -> Result<Self, WorkerError> {
        Err(WorkerError::Unavailable("not implemented".to_string()))
    }

    pub fn handshake(&mut self) -> Result<WorkerInfo, WorkerError> {
        Err(WorkerError::Unavailable("not implemented".to_string()))
    }

    pub fn send(&mut self, _command: &WorkerCommand) -> Result<(), WorkerError> {
        Err(WorkerError::Unavailable("not implemented".to_string()))
    }

    pub fn recv(&mut self) -> Result<WorkerEvent, WorkerError> {
        Err(WorkerError::Unavailable("not implemented".to_string()))
    }

    pub fn request_shutdown(&mut self) -> Result<(), WorkerError> {
        Err(WorkerError::Unavailable("not implemented".to_string()))
    }
}

#[derive(Debug, Error)]
pub enum FrameExportError {
    #[error("roto frame export is not available: {0}")]
    Unavailable(String),
}

#[derive(Clone, Debug)]
pub struct ExportedFrames {
    pub first_frame: i64,
    pub last_frame: i64,
    pub width: u32,
    pub height: u32,
    pub count: usize,
}

pub fn export_frames(
    _source: &dyn MediaSource,
    _range: TimeRange,
    _rate: FrameRate,
    _output_dir: &Path,
    _max_size: (u32, u32),
) -> Result<ExportedFrames, FrameExportError> {
    Err(FrameExportError::Unavailable("not implemented".to_string()))
}
