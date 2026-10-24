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
use effectcraft_render::{FootageSource, RenderOpts, Renderer};
use effectcraft_time::Tick as EcTick;
use filmcraft_frame::VideoFrame;
use filmcraft_media::generators::GeneratorSource;
use filmcraft_media::{FrameRequest, MediaError, MediaInfo, MediaSource, SharedSource};
use filmcraft_project::{ItemId, MediaClip, MediaRef};
use filmcraft_render::SourceProvider;
use filmcraft_time::{FrameRate, Tick};

use crate::BridgeError;
use crate::convert::param_to_value;
use crate::document::{Composition, EffectLink, FrameForgeProject};
use crate::roto::{MaskCache, MaskFrame, default_cache_root};

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
    /// The cached roto mask, or the reason it could not be opened.
    roto: Option<Result<MaskCache, String>>,
}

impl ComposedSource {
    /// Wrap `inner` with `composition`. `seed` identifies the source; it feeds
    /// the effect instances so procedural effects are deterministic per source.
    pub fn new(inner: SharedSource, composition: Composition, seed: u32) -> Self {
        Self { inner, composition, seed, roto: None }
    }

    /// Render this source through `cache`'s alpha masks.
    pub fn with_roto_cache(mut self, cache: MaskCache) -> Self {
        self.roto = Some(Ok(cache));
        self
    }

    /// Remember that the project names a mask cache this host could not open.
    /// Rendering fails with `reason` instead of silently showing unmasked
    /// pixels.
    pub fn with_roto_error(mut self, reason: impl Into<String>) -> Self {
        self.roto = Some(Err(reason.into()));
        self
    }

    /// Fetch the frame, run the effect chain or composition, then multiply the
    /// result's straight alpha by the cached mask. Preview and export use this
    /// same method, so they see the same mask bytes.
    fn apply_roto(&self, frame: VideoFrame, t: Tick) -> Result<VideoFrame, MediaError> {
        let Some(roto) = &self.roto else { return Ok(frame) };
        let cache = match roto {
            Ok(cache) => cache,
            Err(reason) => return Err(MediaError::Unsupported(reason.clone())),
        };
        let Some(mask) = cache.frame(t).map_err(|error| MediaError::Unsupported(error.to_string()))? else {
            return Ok(frame);
        };
        apply_mask(frame, &mask)
    }
}

/// Multiply a straight-alpha RGBA frame's alpha by an 8-bit matte.
///
/// Dimensions must match: a proxy-sized mask silently stretched onto a
/// full-resolution export would be a different picture. The worker regenerates
/// masks at the requested working resolution instead (R3), and the cache key
/// includes that resolution.
fn apply_mask(frame: VideoFrame, mask: &MaskFrame) -> Result<VideoFrame, MediaError> {
    let (width, height) = (frame.width, frame.height);
    if mask.width() != width || mask.height() != height {
        return Err(MediaError::Unsupported(format!("roto mask is {}x{} but the clip frame is {width}x{height}", mask.width(), mask.height())));
    }
    let pixels =
        (width as usize).checked_mul(height as usize).ok_or_else(|| MediaError::Unsupported(format!("roto mask frame size {width}x{height} overflows")))?;
    let alpha = mask.alpha();
    if alpha.len() != pixels {
        return Err(MediaError::Unsupported(format!("roto mask carries {} alpha pixels, expected {pixels}", alpha.len())));
    }
    let expected_bytes = pixels.checked_mul(4).ok_or_else(|| MediaError::Unsupported(format!("roto mask frame size {width}x{height} overflows")))?;
    let mut data = frame.to_rgba8();
    if data.len() != expected_bytes {
        return Err(MediaError::Unsupported(format!("clip frame carries {} RGBA bytes, expected {expected_bytes}", data.len())));
    }
    for (pixel, matte) in data.as_chunks_mut::<4>().0.iter_mut().zip(alpha.iter().copied()) {
        if let Some(a) = pixel.get_mut(3) {
            *a = ((u16::from(*a) * u16::from(matte) + 127) / 255) as u8;
        }
    }
    Ok(VideoFrame::rgba8(width, height, data).with_pts(frame.pts))
}

impl MediaSource for ComposedSource {
    fn info(&self) -> &MediaInfo {
        self.inner.info()
    }

    fn video_frame(&self, req: FrameRequest) -> Result<Arc<VideoFrame>, MediaError> {
        if self.composition.is_noop() && self.roto.is_none() {
            return self.inner.video_frame(req);
        }
        let rate = self.inner.info().frame_rate();
        // A composition with a compositing-engine comp: the comp IS the picture.
        // Its source footage layer reads this clip's media through the effect
        // chain ([`ComposedFootage`]); every layer of the comp — text, shapes,
        // solids, nested comps — renders over it with the compositing engine's
        // own layer compositor.
        let frame = if let (Some(project), Some(comp)) = (&self.composition.ec_project, self.composition.comp) {
            let footage = ComposedFootage {
                source_item: self.composition.source_item,
                inner: self.inner.as_ref(),
                chain: &self.composition.effects,
                rate,
                seed: self.seed,
            };
            let renderer = Renderer::new(project, &footage, RenderOpts::default());
            // Both engines measure time in the same ticks, so the timeline time
            // is the comp time, exactly.
            let img = renderer.comp_frame(comp, EcTick(req.time.0));
            let (w, h) = (img.width, img.height);
            if w == 0 || h == 0 {
                return self.inner.video_frame(req);
            }
            let out = img.to_rgba8();
            VideoFrame::rgba8(w, h, out).with_pts(req.time)
        } else {
            // Effect-chain-only composition: the chain's output is the picture.
            let (buf, w, h) = chain_buf(self.inner.as_ref(), &self.composition.effects, req.time, rate, self.seed)?;
            if w == 0 || h == 0 {
                return self.inner.video_frame(req);
            }
            let out = layer_rgba(&buf, w, h)?;
            VideoFrame::rgba8(w, h, out).with_pts(req.time)
        };
        Ok(Arc::new(self.apply_roto(frame, req.time)?))
    }

    fn audio(&self, start: i64, frames: usize, sample_rate: u32) -> Result<filmcraft_frame::AudioBuffer, MediaError> {
        self.inner.audio(start, frames, sample_rate)
    }
}

/// Fetch the inner source's frame at editing time `t` and run the composition's
/// effect chain over it. Returns the (possibly padded) buffer and the frame's
/// pixel size — the layer rectangle [`layer_rgba`] crops back out.
fn chain_buf(inner: &dyn MediaSource, effects: &[EffectLink], t: Tick, rate: FrameRate, seed: u32) -> Result<(fx::Buf, u32, u32), MediaError> {
    let frame = inner.video_frame(FrameRequest::full(t))?;
    let (w, h) = (frame.width, frame.height);
    let mut buf = fx::Buf { img: FxImage::from_rgba8(w, h, &frame.to_rgba8()), offset: [0.0, 0.0], scale: 1.0 };
    let layer_size = [f64::from(w), f64::from(h)];
    for (i, link) in effects.iter().enumerate() {
        if !link.enabled {
            continue;
        }
        buf = apply_link(link, t, rate, i, seed, layer_size, buf)?;
    }
    Ok((buf, w, h))
}

/// Serves this clip's media — after its effect chain — to a compositing-engine
/// project: the comp's source footage layer over the editing engine's decode
/// pipeline. Other footage items of the project have no picture here (they
/// stay offline); audio passes through interleaved, as the comp's layer audio
/// requests it.
struct ComposedFootage<'a> {
    /// The compositing-engine footage item this clip's media backs.
    source_item: Option<effectcraft_project::ItemId>,
    inner: &'a dyn MediaSource,
    chain: &'a [EffectLink],
    rate: FrameRate,
    seed: u32,
}

impl FootageSource for ComposedFootage<'_> {
    fn frame(&self, item: effectcraft_project::ItemId, _footage: &effectcraft_project::Footage, t: EcTick) -> Option<Arc<FxImage>> {
        if self.source_item != Some(item) {
            return None;
        }
        let (buf, w, h) = chain_buf(self.inner, self.chain, Tick(t.0), self.rate, self.seed).ok()?;
        let out = layer_rgba(&buf, w, h).ok()?;
        Some(Arc::new(FxImage::from_rgba8(w, h, &out)))
    }

    fn audio(&self, item: effectcraft_project::ItemId, _footage: &effectcraft_project::Footage, t: EcTick, frames: usize, rate: u32) -> Option<Vec<f32>> {
        if self.source_item != Some(item) {
            return None;
        }
        let start = Tick(t.0).to_units_floor(i64::from(rate));
        let buf = self.inner.audio(start, frames, rate).ok()?;
        Some(buf.interleaved())
    }
}

/// The sources of a whole project: one [`MediaSource`] per media item the
/// document can supply. Feeds the editing engine's renderer and exporter
/// through their `SourceProvider` seam.
#[derive(Clone)]
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
    /// `MediaInfo`; file media opens through the editing engine's codecs. A
    /// missing or unreadable file becomes the offline slate — the renderer's
    /// "media not found" picture — so a project whose media moved still opens
    /// and renders, and the media plays again once the file is back. An item
    /// with a composition is wrapped in [`ComposedSource`].
    pub fn build(doc: &FrameForgeProject) -> Result<Self, BridgeError> {
        ProjectSources::build_with_roto_root(doc, &default_cache_root())
    }

    /// [`ProjectSources::build`] with an explicit roto cache root (tests and
    /// hosts that keep the cache somewhere other than the default).
    pub fn build_with_roto_root(doc: &FrameForgeProject, root: &std::path::Path) -> Result<Self, BridgeError> {
        ProjectSources::build_from_with_roto_root(&doc.project, &doc.compositions, root)
    }

    /// [`ProjectSources::build`] over an editing project and its composition
    /// map directly — for hosts that keep the editing project inside the
    /// engine's session.
    pub fn build_from(project: &filmcraft_project::Project, compositions: &std::collections::HashMap<ItemId, Composition>) -> Result<Self, BridgeError> {
        ProjectSources::build_from_with_roto_root(project, compositions, &default_cache_root())
    }

    /// [`ProjectSources::build_from`] with an explicit roto cache root.
    pub fn build_from_with_roto_root(
        project: &filmcraft_project::Project,
        compositions: &std::collections::HashMap<ItemId, Composition>,
        root: &std::path::Path,
    ) -> Result<Self, BridgeError> {
        let mut ids = Vec::new();
        project.root.all_items(&mut ids);
        let mut sources = HashMap::new();
        for id in ids {
            let Some((_, clip, _)) = project.resolve_media(id) else { continue };
            let Some(inner) = media_source(clip) else { continue };
            let source: SharedSource = match compositions.get(&id) {
                Some(comp) if !comp.is_noop() => {
                    let mut composed = ComposedSource::new(inner, comp.clone(), id.0 as u32);
                    if let Some(roto) = &comp.roto {
                        composed = match MaskCache::open(root, &roto.key) {
                            Ok(cache) => composed.with_roto_cache(cache),
                            Err(error) => composed.with_roto_error(error.to_string()),
                        };
                    }
                    Arc::new(composed)
                }
                _ => inner,
            };
            sources.insert(id, source);
        }
        Ok(Self { sources })
    }
}

/// The source of a media clip: a synthetic generator (video or audio-only),
/// or a file opened through the editing engine's codecs.
fn media_source(clip: &MediaClip) -> Option<SharedSource> {
    match &clip.media {
        MediaRef::Generator(_) => generator_source(clip).map(|g| Arc::new(g) as SharedSource),
        MediaRef::File { path } => Some(file_source(clip, path)),
    }
}

/// Open `path` through the editing engine's codecs, or the offline slate when
/// the file is missing or cannot be opened.
fn file_source(clip: &MediaClip, path: &str) -> SharedSource {
    let Ok(bytes) = std::fs::read(std::path::Path::new(path)) else {
        return Arc::new(filmcraft_media::OfflineSource { info: clip.info.clone() });
    };
    let name = std::path::Path::new(path).file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned());
    match filmcraft_media::open_bytes(&name, bytes.into(), &filmcraft_codecs::openers()) {
        Ok(source) => source,
        Err(_) => Arc::new(filmcraft_media::OfflineSource { info: clip.info.clone() }),
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

/// Reconstruct the generator source a media clip describes. Audio-only
/// generators (Tone) have no video stream: the size and rate never stand for
/// pixels there, so defaults fill them and the tone plays from the audio side.
fn generator_source(clip: &MediaClip) -> Option<GeneratorSource> {
    let MediaRef::Generator(generator) = &clip.media else { return None };
    let (w, h, rate) = match clip.info.video.as_ref() {
        Some(v) => (v.width, v.height, v.frame_rate),
        None => (320, 240, clip.info.frame_rate()),
    };
    Some(GeneratorSource::new(generator.clone(), w, h, rate, clip.info.duration).with_name(clip.info.name.as_str()))
}
