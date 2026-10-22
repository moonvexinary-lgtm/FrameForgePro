//! Audio playback: a cpal output stream driven by a worker thread that
//! renders the timeline mix through the editing engine's mixer graph. The
//! samples the sound card consumes are the playback clock — the playhead is
//! exactly where the audio is (upstream's design, minimal form).
//!
//! The stream requests the sequence's rate (48 kHz) stereo f32; when the
//! device insists on another rate the worker resamples linearly and the clock
//! still counts device frames, converted exactly through `Tick::from_units`.
//! With no usable device the player reports why and the shell falls back to
//! the wall clock (silent preview).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use filmcraft_project::{ItemId, Project};
use filmcraft_render::audio::mix_sequence;
use filmcraft_time::Tick;
use frameforge_bridge::ProjectSources;

/// The sequence-rate chunk the worker renders at a time (100 ms at 48 kHz).
const CHUNK: usize = 4800;
/// Queued chunks the callback may hold ahead of the playhead.
const QUEUE_DEPTH: usize = 6;
const SEQUENCE_RATE: u32 = 48_000;

/// The callback's queue: the channel end plus the half-consumed chunk.
struct Slot {
    rx: Option<mpsc::Receiver<Vec<f32>>>,
    chunk: Vec<f32>,
    idx: usize,
}

pub struct Player {
    stream: Option<cpal::Stream>,
    slot: Arc<Mutex<Slot>>,
    /// Device frames the sound card consumed since play began.
    played: Arc<AtomicU64>,
    /// The worker rendered to the end.
    finished: Arc<AtomicBool>,
    device_rate: u32,
    channels: u16,
    /// Why there is no stream (shown in the status bar).
    note: Option<String>,
}

impl Player {
    pub fn new() -> Player {
        let note = device_config().err();
        Player {
            stream: None,
            slot: Arc::new(Mutex::new(Slot { rx: None, chunk: Vec::new(), idx: 0 })),
            played: Arc::new(AtomicU64::new(0)),
            finished: Arc::new(AtomicBool::new(true)),
            device_rate: note.is_none().then_some(()).and_then(|_| device_config().ok()).map(|(r, _)| r).unwrap_or(SEQUENCE_RATE),
            channels: 2,
            note,
        }
    }

    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    /// The playback position as ticks: exactly the audio the device consumed.
    pub fn position(&self) -> Tick {
        Tick::from_units(self.played.load(Ordering::Relaxed) as i64, i64::from(self.device_rate))
    }

    /// True when the mix has been rendered to the end and the queue is drained.
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Relaxed) && self.slot.lock().unwrap_or_else(|e| e.into_inner()).rx.is_none()
    }

    /// Start playing sequence `seq` of `project` from `start` until `end`.
    /// Returns why playback is impossible (the caller falls back to the wall
    /// clock — honestly, silently).
    pub fn play(&mut self, project: Arc<Project>, sources: ProjectSources, seq: ItemId, start: Tick, end: Tick) -> Result<(), String> {
        self.stop();
        if let Some(why) = &self.note {
            return Err(why.clone());
        }
        if self.stream.is_none() {
            let (_, channels) = device_config().map_err(|e| e)?; // note is None here, so this succeeds
            self.channels = channels;
            let stream = build_stream(self.channels, Arc::clone(&self.slot), Arc::clone(&self.played))?;
            stream.play().map_err(|e| format!("audio: {e}"))?;
            self.stream = Some(stream);
        }
        let (tx, rx) = mpsc::sync_channel::<Vec<f32>>(QUEUE_DEPTH);
        {
            let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
            slot.rx = Some(rx);
            slot.chunk = Vec::new();
            slot.idx = 0;
        }
        self.played.store(0, Ordering::Relaxed);
        self.finished.store(false, Ordering::Relaxed);

        let rate = u64::from(SEQUENCE_RATE);
        let start_sample = start.to_units_floor(i64::from(SEQUENCE_RATE)) as u64;
        let end_sample = end.to_units_floor(i64::from(SEQUENCE_RATE)) as u64;
        let device_rate = u64::from(self.device_rate);
        let channels = self.channels;
        let finished = Arc::clone(&self.finished);

        std::thread::spawn(move || {
            let mut sent = start_sample;
            while sent < end_sample {
                let Some(seq_ref) = project.items.get(&seq).and_then(|it| it.as_sequence()) else { break };
                let frames = CHUNK.min((end_sample - sent) as usize);
                let mixed = mix_sequence(&project, seq_ref, sent as i64, frames, &sources);
                let stereo = mixed.interleaved();
                let out = if device_rate == rate { stereo } else { resample(&stereo, rate, device_rate) };
                let out = if channels == 2 { out } else { stereo_pairs(&out, channels) };
                if tx.send(out).is_err() {
                    break; // stopped while rendering
                }
                sent += frames as u64;
            }
            finished.store(true, Ordering::Relaxed);
        });
        Ok(())
    }

    /// Stop: dropping the receiver silences the device and ends the worker.
    pub fn stop(&mut self) {
        let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        slot.rx = None;
        slot.chunk = Vec::new();
        slot.idx = 0;
        self.finished.store(true, Ordering::Relaxed);
    }
}

impl Default for Player {
    fn default() -> Self {
        Player::new()
    }
}

/// The output stream config to request: f32, stereo preferred, the sequence
/// rate where the device supports it, else the device default.
fn device_config() -> Result<(u32, u16), String> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or_else(|| "no audio output device".to_string())?;
    let want = SEQUENCE_RATE;
    let exact = device
        .supported_output_configs()
        .ok()
        .and_then(|it| {
            it.filter(|c| c.channels() == 2 && c.sample_format() == cpal::SampleFormat::F32).find_map(|c| c.try_with_sample_rate(cpal::SampleRate(want)))
        })
        .map(|c| c.config());
    let Some(chosen) = exact else {
        return Err("audio device has no 48 kHz f32 stereo mode".to_string());
    };
    Ok((chosen.sample_rate.0, chosen.channels))
}

fn build_stream(channels: u16, slot: Arc<Mutex<Slot>>, played: Arc<AtomicU64>) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or_else(|| "no audio output device".to_string())?;
    let config = device
        .supported_output_configs()
        .ok()
        .and_then(|it| {
            it.filter(|c| c.channels() == 2 && c.sample_format() == cpal::SampleFormat::F32)
                .find_map(|c| c.try_with_sample_rate(cpal::SampleRate(SEQUENCE_RATE)))
        })
        .map(|c| c.config())
        .ok_or_else(|| "audio device has no 48 kHz f32 stereo mode".to_string())?;
    let ch = usize::from(channels.max(1));
    let stream = device
        .build_output_stream(
            &config,
            move |buf: &mut [f32], _| {
                let mut slot = slot.lock().unwrap_or_else(|e| e.into_inner());
                let mut consumed = 0u64;
                for frame in buf.chunks_mut(ch) {
                    let mut filled = false;
                    while slot.rx.is_some() {
                        if slot.idx + ch <= slot.chunk.len() {
                            for (i, s) in frame.iter_mut().enumerate() {
                                *s = slot.chunk[slot.idx + i.min(ch - 1)];
                            }
                            slot.idx += ch;
                            consumed += 1;
                            filled = true;
                            break;
                        }
                        match slot.rx.as_mut().map(|rx| rx.try_recv()) {
                            Some(Ok(chunk)) => {
                                slot.chunk = chunk;
                                slot.idx = 0;
                            }
                            Some(Err(mpsc::TryRecvError::Empty)) => break, // underrun: silence this frame
                            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                                slot.rx = None;
                                slot.chunk = Vec::new();
                                slot.idx = 0;
                                break;
                            }
                            None => break,
                        }
                    }
                    if !filled {
                        frame.fill(0.0);
                    }
                }
                if consumed > 0 {
                    played.fetch_add(consumed, Ordering::Relaxed);
                }
            },
            |_err| {},
            None,
        )
        .map_err(|e| format!("audio stream: {e}"))?;
    Ok(stream)
}

/// Linear resample of interleaved stereo from `from` to `to` Hz.
fn resample(stereo: &[f32], from: u64, to: u64) -> Vec<f32> {
    let in_frames = stereo.len() / 2;
    if in_frames == 0 || from == to {
        return stereo.to_vec();
    }
    let out_frames = (in_frames as f64 * to as f64 / from as f64).round() as usize;
    let mut out = Vec::with_capacity(out_frames * 2);
    for i in 0..out_frames {
        let pos = i as f64 * from as f64 / to as f64;
        let i0 = pos.floor() as usize;
        let frac = (pos - i0 as f64) as f32;
        let i1 = (i0 + 1).min(in_frames - 1);
        for c in 0..2 {
            let a = stereo[i0 * 2 + c];
            let b = stereo[i1 * 2 + c];
            out.push(a + (b - a) * frac);
        }
    }
    out
}

/// Convert 2-channel interleaved audio to `channels` channels (1 = mono
/// downmix; more = stereo on the first pair, silence elsewhere).
fn stereo_pairs(stereo: &[f32], channels: u16) -> Vec<f32> {
    let frames = stereo.len() / 2;
    let ch = channels.max(1) as usize;
    let mut out = vec![0.0f32; frames * ch];
    for f in 0..frames {
        let (l, r) = (stereo[f * 2], stereo[f * 2 + 1]);
        if ch == 1 {
            out[f] = (l + r) / 2.0;
        } else {
            out[f * ch] = l;
            out[f * ch + 1] = r;
        }
    }
    out
}
