//! The composed media source and the project-wide source provider.
//!
//! [`ComposedSource`] wraps any `MediaSource` and runs its frames through a
//! composition's chain of real EffectCraft effects ([`Composition`]) before
//! the editing engine's renderer reads them. Audio passes through untouched.
//! Because the source is a plain `MediaSource`, the renderer, the monitors and
//! the export pipeline all see the same composited picture through the same
//! code path — there is no second renderer.
//!
//! [`ProjectSources`] builds the sources of a whole [`FrameForgeProject`]:
//! generator media is reconstructed from the project's own `MediaInfo`, and
//! items with a composition are wrapped in [`ComposedSource`].

use std::collections::HashMap;
use std::sync::Arc;

use effectcraft_effects as fx;
use effectcraft_raster::Image as FxImage;
use filmcraft_frame::VideoFrame;
use filmcraft_media::generators::GeneratorSource;
use filmcraft_media::{FrameRequest, MediaError, MediaInfo, MediaSource, SharedSource};
use filmcraft_project::{ItemId, MediaClip, MediaRef};
use filmcraft_render::SourceProvider;
use filmcraft_time::{FrameRate, Tick};

use crate::BridgeError;
use crate::convert::param_to_value;
use crate::document::{Composition, EffectLink, FrameForgeProject};

/// Runs one effect of a composition.
///
/// Parameters are evaluated at media time `t` (exact ticks) and moved into the
/// effect's parameter set; parameter ids the effect does not know are skipped
/// (kept in the document for forward compatibility). The whole call is a pure
/// function of `(link, t, buf)` — no hidden state, so scrubbing in any
/// direction gives identical frames.
fn apply_link(link: &EffectLink, t: Tick, rate: FrameRate, index: usize, seed: u32, layer_size: [f64; 2], buf: fx::Buf) -> Result<fx::Buf, MediaError> {
    let Some(spec) = fx::find(&link.effect) else {
        return Err(MediaError::Unsupported(format!("EffectCraft effect '{}' was not found in the effect registry", link.effect)));
    };
    // Start from the effect's own defaults (point parameters are relative to the
    // layer size), then overlay the document's values evaluated at `t`.
    let mut params = fx::Params { values: spec.params.iter().map(|p| (p.id.to_string(), fx::default_value(p, layer_size))).collect() };
    for (id, param) in &link.params {
        if !params.values.contains_key(id) {
            continue;
        }
        if let Some(v) = param_to_value(&param.value_at(t)) {
            params.values.insert(id.clone(), v);
        }
    }
    let seconds = t.seconds();
    let env = fx::EffectEnv { comp_time: seconds, frame_rate: rate.as_f64(), ..fx::EffectEnv::default() };
    let ctx = fx::EffectCtx { params: &params, time: seconds, layer_size, seed: seed.wrapping_add(index as u32), adjustment: false, env };
    Ok(fx::apply(spec, &ctx, buf))
}

/// A media source whose video frames pass through a compositing composition
/// before the editing engine reads them.
pub struct ComposedSource {
    inner: SharedSource,
    composition: Composition,
    /// Stable per source, so procedural effects re-render identically.
    seed: u32,
}

impl ComposedSource {
    /// Wrap `inner` with `composition`. `seed` identifies the source; it feeds
    /// the effect instances so procedural effects are deterministic per source.
    pub fn new(inner: SharedSource, composition: Composition, seed: u32) -> Self {
        Self { inner, composition, seed }
    }
}

impl MediaSource for ComposedSource {
    fn info(&self) -> &MediaInfo {
        self.inner.info()
    }

    fn video_frame(&self, req: FrameRequest) -> Result<Arc<VideoFrame>, MediaError> {
        let frame = self.inner.video_frame(req)?;
        if self.composition.is_noop() {
            return Ok(frame);
        }
        let (w, h) = (frame.width, frame.height);
        if w == 0 || h == 0 {
            return Ok(frame);
        }
        // RGBA8 in, EffectCraft premultiplied f32, effects, RGBA8 back out.
        let rgba = frame.to_rgba8();
        let mut buf = fx::Buf { img: FxImage::from_rgba8(w, h, &rgba), offset: [0.0, 0.0], scale: 1.0 };
        let layer_size = [f64::from(w), f64::from(h)];
        let rate = self.inner.info().frame_rate();
        for (i, link) in self.composition.effects.iter().enumerate() {
            if !link.enabled {
                continue;
            }
            buf = apply_link(link, req.time, rate, i, self.seed, layer_size, buf)?;
        }
        let out = layer_rgba(&buf, w, h)?;
        Ok(Arc::new(VideoFrame::rgba8(w, h, out).with_pts(req.time)))
    }

    fn audio(&self, start: i64, frames: usize, sample_rate: u32) -> Result<filmcraft_frame::AudioBuffer, MediaError> {
        self.inner.audio(start, frames, sample_rate)
    }
}

/// The sources of a whole project: one [`MediaSource`] per media item the
/// document can supply. Feeds the editing engine's renderer and exporter
/// through their `SourceProvider` seam.
pub struct ProjectSources {
    sources: HashMap<ItemId, SharedSource>,
}

impl SourceProvider for ProjectSources {
    fn source(&self, item: ItemId) -> Option<SharedSource> {
        self.sources.get(&item).cloned()
    }
}

impl ProjectSources {
    /// Build the sources of `doc`.
    ///
    /// Synthetic generator media is reconstructed from the project's own
    /// `MediaInfo` (no files, nothing to download); an item with a composition
    /// is wrapped in [`ComposedSource`]. File-based media is not opened at this
    /// milestone: the renderer draws its offline slate for such items until
    /// file import lands (see `docs/ARCHITECTURE.md`, M3).
    pub fn build(doc: &FrameForgeProject) -> Result<Self, BridgeError> {
        let mut ids = Vec::new();
        doc.project.root.all_items(&mut ids);
        let mut sources = HashMap::new();
        for id in ids {
            let Some((_, clip, _)) = doc.project.resolve_media(id) else { continue };
            let Some(inner) = generator_source(clip) else { continue };
            let source: SharedSource = match doc.compositions.get(&id) {
                Some(comp) if !comp.is_noop() => Arc::new(ComposedSource::new(Arc::new(inner), comp.clone(), id.0 as u32)),
                _ => Arc::new(inner),
            };
            sources.insert(id, source);
        }
        Ok(Self { sources })
    }
}

/// Extracts the original `w` × `h` layer rectangle (layer pixels) from a buffer
/// the effect chain may have grown or moved, as straight RGBA8.
///
/// Effects that keep the layer's geometry (blur, colour, stylize…) pad the
/// buffer with transparent pixels so light can leave the picture and the edges
/// soften correctly, the way the compositing engine composites the same
/// buffer: the layer rectangle is then a sub-rectangle of the buffer, and this
/// crops it back out — soft alpha edges included. Effects that change the
/// geometry (`scale`, non-finite offsets, out-of-bounds rectangles) are
/// reported as unsupported rather than rendered wrongly.
fn layer_rgba(buf: &fx::Buf, w: u32, h: u32) -> Result<Vec<u8>, MediaError> {
    let unsupported = |what: &str| Err(MediaError::Unsupported(format!("EffectCraft effect {what}; not supported by the composed-source bridge yet")));
    if !buf.scale.is_finite() || buf.scale != 1.0 {
        return unsupported("changed the layer scale");
    }
    if !buf.offset[0].is_finite() || !buf.offset[1].is_finite() {
        return unsupported("moved the layer to a non-finite offset");
    }
    let (bw, bh) = (buf.img.width, buf.img.height);
    let (x0, y0) = (buf.offset[0].round(), buf.offset[1].round());
    let (x1, y1) = (x0 + f64::from(w), y0 + f64::from(h));
    if x0 < 0.0 || y0 < 0.0 || x1 > f64::from(bw) || y1 > f64::from(bh) {
        return unsupported("moved the layer rectangle outside the buffer");
    }
    let (x0, y0) = (x0 as usize, y0 as usize);
    let full = buf.img.to_rgba8();
    let stride = bw as usize * 4;
    let row_bytes = w as usize * 4;
    let mut out = vec![0u8; row_bytes * h as usize];
    for row in 0..h as usize {
        let src = (y0 + row) * stride + x0 * 4;
        let dst = row * row_bytes;
        let (Some(src_row), Some(dst_row)) = (full.get(src..src + row_bytes), out.get_mut(dst..dst + row_bytes)) else {
            return unsupported("left the buffer smaller than the layer rectangle");
        };
        dst_row.copy_from_slice(src_row);
    }
    Ok(out)
}

/// Reconstruct the generator source a media clip describes, when it describes
/// one with a video stream (a tone-only generator has none; its audio is
/// opened with file media in a later milestone).
fn generator_source(clip: &MediaClip) -> Option<GeneratorSource> {
    let MediaRef::Generator(generator) = &clip.media else { return None };
    let video = clip.info.video.as_ref()?;
    Some(GeneratorSource::new(generator.clone(), video.width, video.height, video.frame_rate, clip.info.duration).with_name(clip.info.name.as_str()))
}
