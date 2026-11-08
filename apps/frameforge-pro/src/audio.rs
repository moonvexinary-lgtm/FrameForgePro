//! Audio playback: a cpal output stream driven by a worker thread that
//! renders through the editing engine's mixer graph. The samples the sound card
//! consumes are the playback clock — the playhead is exactly where the audio is
//! (upstream's design, minimal form). Two mixes share one stream: the Program
//! monitor plays the sequence ([`Player::play`]) and the Source monitor plays
//! one media item's own sound ([`Player::play_item`]).
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
use filmcraft_render::SourceProvider;
use filmcraft_render::audio::mix_sequence;
use filmcraft_time::Tick;
use frameforge_bridge::ProjectSources;

/// The sequence-rate chunk the worker renders at a time (100 ms at 48 kHz).
const CHUNK: usize = 4800;
/// Queued chunks the callback may hold ahead of the playhead.
const QUEUE_DEPTH: usize = 6;
const SEQUENCE_RATE: u32 = 48_000;

/// Whether `item` can make sound for Source-monitor auditioning: media that is
/// not offline and declares an audio stream. Pure, so the transport's decision
/// to play with sound or fall back to silence is testable without a device.
pub fn item_has_audio(project: &Project, item: ItemId) -> bool {
    project.item(item).and_then(|it| it.as_media()).is_some_and(|media| !media.offline && media.info.has_audio())
}

/// `frames` interleaved stereo samples of `item`'s own media audio starting at
/// `start_sample` (the sequence rate), for Source-monitor auditioning. Silence
/// when the item has no source or the decode fails: auditioning never edits the
/// project and never panics on a missing or unsupported file. A source with more
/// than two channels is monitored through its first pair — the Program monitor
/// owns the full downmix.
pub fn item_audio(sources: &ProjectSources, item: ItemId, start_sample: i64, frames: usize) -> Vec<f32> {
    let Some(source) = sources.source(item) else {
        return vec![0.0; frames * 2];
    };
    let Ok(buffer) = source.audio(start_sample, frames, SEQUENCE_RATE) else {
        return vec![0.0; frames * 2];
    };
    let count = buffer.frames();
    let left = buffer.channels.first();
    let right = buffer.channels.get(1);
    let mut out = Vec::with_capacity(count * 2);
    for i in 0..count {
        let l = left.and_then(|c| c.get(i)).copied().unwrap_or(0.0);
        let r = right.and_then(|c| c.get(i)).copied().unwrap_or(l);
        out.push(l);
        out.push(r);
    }
    out
}

/// Render `[start_sample, end_sample)` of `mix` (interleaved stereo at the
/// sequence rate), converted to the device's rate and channel count and handed
/// to `emit` one chunk at a time until `emit` refuses or the range ends. The
/// sound card's callback is `emit` in production; a test passes a local
/// collector, so the whole render/convert queue is exercised without a device.
fn render_into(
    mix: impl Fn(i64, usize) -> Vec<f32>,
    start_sample: u64,
    end_sample: u64,
    device_rate: u64,
    channels: u16,
    mut emit: impl FnMut(Vec<f32>) -> bool,
) {
    let rate = u64::from(SEQUENCE_RATE);
    let mut sent = start_sample;
    while sent < end_sample {
        let frames = CHUNK.min((end_sample - sent) as usize);
        let stereo = mix(sent as i64, frames);
        let out = if device_rate == rate { stereo } else { resample(&stereo, rate, device_rate) };
        let out = if channels == 2 { out } else { stereo_pairs(&out, channels) };
        if !emit(out) {
            break;
        }
        sent += frames as u64;
    }
}

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
    /// Build a silent, unopened player. The audio device is only touched by
    /// `play`, so constructing the app (and every test that builds one) never
    /// opens WASAPI and cannot crash or stall on a machine without a device.
    pub fn new() -> Player {
        Player {
            stream: None,
            slot: Arc::new(Mutex::new(Slot { rx: None, chunk: Vec::new(), idx: 0 })),
            played: Arc::new(AtomicU64::new(0)),
            finished: Arc::new(AtomicBool::new(true)),
            device_rate: SEQUENCE_RATE,
            channels: 2,
            note: None,
        }
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
        self.start(start, end, move |sent, frames| {
            let Some(seq_ref) = project.items.get(&seq).and_then(|it| it.as_sequence()) else { return Vec::new() };
            mix_sequence(&project, seq_ref, sent, frames, &sources).interleaved()
        })
    }

    /// Play one media item's own sound, for the Source monitor. Same stream,
    /// clock and fallback as [`Player::play`]; only the mix differs, so a Source
    /// audition never runs through the sequence's tracks and effects.
    pub fn play_item(&mut self, sources: ProjectSources, item: ItemId, start: Tick, end: Tick) -> Result<(), String> {
        self.start(start, end, move |sent, frames| item_audio(&sources, item, sent, frames))
    }

    /// The shared playback start: open the device on first use, queue chunks
    /// rendered by `mix`, and count the device frames consumed as the clock.
    fn start(&mut self, start: Tick, end: Tick, mix: impl Fn(i64, usize) -> Vec<f32> + Send + 'static) -> Result<(), String> {
        self.stop();
        if let Some(why) = &self.note {
            return Err(why.clone());
        }
        if self.stream.is_none() {
            // The device is opened on the first real play, not at construction.
            let (rate, channels) = match device_config() {
                Ok(cfg) => cfg,
                Err(e) => {
                    self.note = Some(e.clone());
                    return Err(e);
                }
            };
            self.device_rate = rate;
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

        let start_sample = start.to_units_floor(i64::from(SEQUENCE_RATE)) as u64;
        let end_sample = end.to_units_floor(i64::from(SEQUENCE_RATE)) as u64;
        let device_rate = u64::from(self.device_rate);
        let channels = self.channels;
        let finished = Arc::clone(&self.finished);

        std::thread::spawn(move || {
            render_into(mix, start_sample, end_sample, device_rate, channels, |chunk| tx.send(chunk).is_ok());
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use filmcraft_media::generators::GeneratorSource;
    use filmcraft_media::{Generator, MediaSource};
    use filmcraft_project::{ItemId, ItemKind, Label, MediaClip, MediaRef, Project};
    use filmcraft_time::{FrameRate, TICKS_PER_SECOND, Tick};
    use frameforge_bridge::ProjectSources;

    use super::{item_audio, item_has_audio, render_into};

    /// A media item backed by `generator`, added to `project`.
    fn media_item(project: &mut Project, generator: Generator) -> ItemId {
        let source = GeneratorSource::new(generator.clone(), 64, 36, FrameRate::FPS_24, Tick(2 * TICKS_PER_SECOND));
        let info = source.info().clone();
        project.add_item(
            "fixture",
            Label::Iris,
            ItemKind::Media(MediaClip {
                media: MediaRef::Generator(generator),
                info,
                interpret: Default::default(),
                mark_in: None,
                mark_out: None,
                markers: Vec::new(),
                offline: false,
                proxy: None,
                identity: None,
            }),
            None,
        )
    }

    fn sources(project: &Project) -> ProjectSources {
        ProjectSources::build_from(project, &HashMap::new()).expect("build sources for the fixture")
    }

    #[test]
    fn item_audio_renders_the_media_sound() {
        let mut project = Project::new("audio");
        let tone = media_item(&mut project, Generator::Tone { hz: 440.0, db: -12.0 });
        let sources = sources(&project);

        assert!(item_has_audio(&project, tone), "a Tone generator has an audio stream");
        let out = item_audio(&sources, tone, 0, 4_800);
        assert_eq!(out.len(), 9_600, "4 800 stereo frames");
        assert!(out.iter().any(|s| s.abs() > 1e-4), "the media sound is not silence");
    }

    #[test]
    fn item_audio_is_silent_without_a_source() {
        let mut project = Project::new("audio");
        let _ = media_item(&mut project, Generator::Tone { hz: 440.0, db: -12.0 });
        let sources = sources(&project);

        let out = item_audio(&sources, ItemId(9_999), 0, 1_000);
        assert_eq!(out.len(), 2_000, "a missing source still fills the request");
        assert!(out.iter().all(|s| *s == 0.0), "a missing source plays silence, never a panic");
    }

    #[test]
    fn item_has_audio_follows_the_media_info() {
        let mut project = Project::new("audio");
        let tone = media_item(&mut project, Generator::Tone { hz: 440.0, db: -12.0 });
        let black = media_item(&mut project, Generator::BlackVideo);

        assert!(item_has_audio(&project, tone), "Tone is audible");
        assert!(!item_has_audio(&project, black), "Black Video has no audio stream");
        assert!(!item_has_audio(&project, ItemId(9_999)), "a missing item is not audible");
    }

    #[test]
    fn render_into_streams_the_whole_range_in_chunks() {
        let mut frames = 0usize;
        let mut sizes = Vec::new();
        render_into(
            |_sent, want| vec![1.0; want * 2],
            0,
            10_000,
            48_000,
            2,
            |chunk| {
                frames += chunk.len() / 2;
                sizes.push(chunk.len() / 2);
                true
            },
        );

        assert_eq!(frames, 10_000, "every requested frame reaches the device");
        assert_eq!(sizes, vec![4_800, 4_800, 400], "100 ms chunks, then the remainder");
    }

    #[test]
    fn render_into_resamples_and_remaps_channels() {
        let mut samples = Vec::new();
        render_into(
            |_sent, want| vec![0.5; want * 2],
            0,
            1_000,
            24_000,
            1,
            |chunk| {
                samples.extend(chunk);
                true
            },
        );

        assert_eq!(samples.len(), 500, "48 kHz stereo in, 24 kHz mono out");
        assert!(samples.iter().all(|s| (*s - 0.5).abs() < 1e-6), "the level survives the resample");
    }
}
