# FrameForge Pro — Premiere Pro Feature Parity Matrix

**Research date:** 2026-10-12
**Premiere version documented:** Adobe Premiere Pro desktop, current release 26.5 (September 2026); release-notes history covers 25.0 (Oct 2024) → 26.5.2 (Sept 2026).
**Sources:** Official Adobe helpx documentation only (URLs cited per section). All raw pages archived at `research/adobe/raw/` with a fetch manifest (`research/adobe/raw/MANIFEST.md`); heading/table extractions at `research/adobe/extracted-headings.md` and `research/adobe/extracted-tables.md`.

**Method & honesty rules**
- Every effect/feature listed below was verified on an official Adobe helpx page fetched on 2026-10-12.
- Where the current Adobe page format (image-card grids) prevented machine extraction, the gap is stated explicitly rather than filled from memory.
- "Kdenlive today" column reflects the *local* Kdenlive 26.11.70-dev source tree (commit `fed4998`) — specifically `data/effects/`, `data/transitions/`, `src/` modules — not marketing claims.
- Premiere third-party plugins and cloud-only services are marked as such; FrameForge does not claim compatibility with them.
- Status column: `☐ Not started` for all items at Milestone 0. Priorities: P0 (core editing), P1 (essential pro), P2 (advanced), P3 (later/defer).

---

## A. Timeline editing

Docs hub: https://helpx.adobe.com/premiere/desktop/edit-projects.html (section tree verified 2026-10-12)

| Feature | Documented behavior (official page) | Kdenlive today | FrameForge plan | Pri | Status |
|---|---|---|---|---|---|
| Multitrack video+audio timeline | "Create a sequence", "Add tracks", "Delete tracks", "Rename tracks", "Edit track appearance" | ✔ TimelineItemModel/TrackModel, unlimited A/V tracks | Reuse Kdenlive timeline model; verify on Windows | P0 | ☐ |
| Source/Program monitors | "Overview of Source Monitor and Program Monitor", display quality, magnification, gang, safe zones, guides, dropped-frame indicator, metadata overlays | ✔ `src/monitor/` (Monitor class, MonitorManager) | Reuse; benchmark scrub latency vs baseline | P0 | ☐ |
| Cut/split/delete | "Cut clips", "Remove clips from a sequence", "Lift and paste frames" | ✔ via timeline2 operations + undo | Reuse | P0 | ☐ |
| Ripple edit | "Perform ripple edits" | ✔ (ripple/roll tooling in timeline2) | Reuse; add tests | P0 | ☐ |
| Roll edit | "Perform rolling edits" | ✔ | Reuse; add tests | P0 | ☐ |
| Slip edit | "Perform slip edits" | ✔ | Reuse; add tests | P0 | ☐ |
| Slide edit | "Perform slide edits" | ✔ | Reuse; add tests | P0 | ☐ |
| Trim mode | "About Trim mode", "Edit in Trim mode", "Asymmetrical trimming", "Perform J-K-L dynamic trimming" | Partial (trim via mouse/keys; dedicated trim-mode UI weaker) | Improve UX on top of existing model | P1 | ☐ |
| J-cuts / L-cuts | "Perform J cuts and L cuts" | ✔ (audio/video link offsets) | Reuse; verify frame accuracy | P0 | ☐ |
| Snapping | "Snap clips" | ✔ SnapModel (`src/timeline2/model/snapmodel.*`) | Reuse | P0 | ☐ |
| Markers | "Overview of markers", add/copy/find/move/delete, colors, comments | ✔ (markers in doc model) | Reuse | P0 | ☐ |
| Track locking | "Track Lock to prevent changes" | ✔ | Reuse | P0 | ☐ |
| Sync lock | "Sync Lock to prevent changes" | ✔ (track flags) | Reuse | P0 | ☐ |
| Track targeting / source patching | "Work with clips on the timeline using Track Targeting", "Add media to the timeline using Source Patching" | ✔ | Reuse | P0 | ☐ |
| Clip grouping | "Group clips" | ✔ GroupsModel | Reuse | P1 | ☐ |
| Clip enable/disable | "Enable or disable a clip" | ✔ | Reuse | P0 | ☐ |
| Nested sequences | "About nested sequences", "Nest a sequence in another sequence" | ✔ | Reuse | P1 | ☐ |
| Adjustment layers | "Create adjustment layers" | ✔ (adjustment-clip asset) | Reuse | P1 | ☐ |
| Speed change (Speed/Duration, Rate Stretch) | "Different ways to change clip speed and duration" | ✔ `speed.xml` + warp speed; TimeRemap class in Core | Reuse; extend | P0 | ☐ |
| Time remapping (variable speed) | "Change clip speed and duration using Time Remapping" | ✔ TimeRemap (`src/` core.h lists TimeRemap) | Reuse; verify keyframe UI parity | P1 | ☐ |
| Freeze frame | "Freeze a video frame for the duration of a clip", "Freeze a frame … using Time Remapping" | ✔ `freeze.xml` effect | Reuse | P1 | ☐ |
| Time interpolation (optical flow etc.) | "Apply Time Interpolation Methods to adjust clip speed" | Partial (frame-blending options depend on MLT) | Implement: frame-sample + blend; evaluate optical flow later | P2 | ☐ |
| Reverse playback of clip | Documented under speed/duration (Reverse Speed checkbox) | ✔ (MLT timewarp supports reverse) | Reuse | P1 | ☐ |
| Multicam editing | "Create a multi-camera source sequence", "Create and edit a multi-camera target sequence", sync by timecode/audio, camera order | Partial (Kdenlive has multicam via QML timeline; less integrated) | Implement on existing model (P2) | P2 | ☐ |
| Proxy workflow | "Ingest and Proxy workflow", "Create proxies", attach/detach, offline clips | ✔ proxy generation via jobs; `externalproxies.rc` | Reuse; Windows-native proxy encode later | P0 | ☐ |
| Offline editing / relink | "Create offline clips", "Relink offline clips", "Locate and link offline files" | ✔ | Reuse | P1 | ☐ |
| Autosave & recovery | Auto Save preferences; "Recover projects after a crash" | ✔ (Kdenlive autosave + `.kdenlive` backup) | Reuse; crash-safe test on Windows | P0 | ☐ |
| Undo/redo & History | "View or make changes in the History panel" | ✔ DocUndoStack + undohelper | Reuse | P0 | ☐ |
| Subclips | "Create a subclip from the Project panel / from the Timeline" | ✔ | Reuse | P1 | ☐ |
| Text-based editing | "Overview of Text-Based editing", transcribe, edit sequences from transcript, pause detection, speaker names, Paper Edit | ✔ TextBasedEdit class exists in Kdenlive (uses VOSK/whisper backends) | Reuse; improve accuracy/models optional | P2 | ☐ |
| Scene Edit Detection | "Detect edit points using Scene Edit Detection" | **✔ audit correction:** already shipped — `SceneSplitTask` (`src/jobs/scenesplittask.*`, 2021): bin-clip action, dialog w/ threshold, markers/range-markers/subclips, min duration, marker category; runs system FFmpeg `select='gt(scene,X)',showinfo`, parses `pts_time:` into markers/zones. **Verified 2026-10-12 against local FFmpeg 8.1.2: command exits 0 and emits exactly the `[Parsed_showinfo`/`pts_time:` tokens the parser expects (tests/small.mkv).** | Reuse | P2 | ✔ Verified-compatible |
| Sequence Index (navigation) | "Navigate timelines with Sequence Index" | ✘ | Defer | P3 | ☐ |
| Project archive/consolidate | "Consolidate and archive projects", "Copy, transcode, or archive project" | Partial (project archiver exists) | Reuse/extend | P2 | ☐ |
| Aspect-ratio handling | "Aspect ratios", "Frame aspect ratio", "Pixel aspect ratio", "Interpret Footage" | ✔ profiles + interpret | Reuse | P1 | ☐ |
| Import formats | "Supported file formats" page (see §H) | ✔ via MLT/FFmpeg | Reuse; add Media Foundation path per format matrix | P0 | ☐ |

---

## B. Video effects (documented inventory)

Sources: category pages under https://helpx.adobe.com/premiere/desktop/add-video-effects/effects-and-transitions-library/ (all fetched 2026-10-12, archived). Note: the GPU-accelerated / 32-bit / YUV classification list pages (`types-of-effects/list-of-accelerated-effects.html`, `list-of-32-bit-effects.html`, `list-of-yuv-effects.html`) currently render as image-card grids that could not be machine-extracted; raw pages archived; classification per effect remains **unverified** and will be re-checked when the pages are readable.

### B1. Adjust (`adjust-effects.html`)
| Effect | Documented behavior | Kdenlive today | FrameForge plan | Pri | Status |
|---|---|---|---|---|---|
| Extract | **Adobe (verified, archived docs):** "removes colors from your video, turning it into grayscale. It makes all pixels with luminance value less than the Black Input Level or greater than the White Input Level appear black, while everything in between shows up as gray or white." | ✘ (no MLT/FFmpeg service; the frei0r channel extractors are unrelated) | **Native FrameForge DSP implemented (M0, slice 8): `FrameForge::Video::Extract` (`src/frameforge/video/extract.*`) — Rec.709 luminance window via 256-entry LUT; black/white input levels + softness ramps + invert; grayscale output, alpha untouched; Catch2 tests (window/invert/clamp) — UNVERIFIED until M1 | P2 | ◐ DSP implemented, untested |
| Levels | "precise control over brightness and contrast… combines Color Balance, Gamma Correction, Brightness and Contrast, and Invert" (behavior quote verified from archived docs; the current page keeps parameter enumeration in images) | ✘ XML-side (MLT `lift_gamma_gain` covers related math) | **Native FrameForge DSP implemented (M0, slice 7): `FrameForge::Video::Levels` (`src/frameforge/video/`) — classic Levels transform (input black/white, gamma, output black/white; RGB linked, alpha untouched) via a 256-entry LUT, behind `IVideoEffect` (`src/frameforge/interfaces/ivideoeffect.h`, packed-RGBA8 v1 contract), Catch2 tests (`tests/frameforge_videodsptest.cpp`): identity-at-defaults, formula anchors, clamping, degenerate-range threshold, stride handling — UNVERIFIED until the M1 build; UI + playback wiring at M3/M6 | P1 | ◐ DSP implemented, untested |
| Lighting Effects | Up to 5 spot lights on clip | ✘ | Implement (GPU) | P3 | ☐ |
| ProcAmp | Brightness/contrast/saturation/hue | ✔ (`brightness.xml`, `lift_gamma_gain.xml`) | Reuse + native GPU equivalent | P1 | ☐ |

### B2. Blur & Sharpen (`blur-and-sharpen-effects.html`)
| Effect | Documented | Kdenlive | Plan | Pri | Status |
|---|---|---|---|---|---|
| Camera Blur (Windows only) | Blur simulating defocus | ✔ conceptually (Gaussian/Direct Blur cover defocus; no dedicated effect name) | Reuse | P2 | ☐ |
| Directional Blur | Angle+length blur | **✔ audit correction:** `avfilter_dblur` (Directional Blur) | Reuse | P1 | ☐ |
| Gaussian Blur | Radius blur | ✔ `box_blur.xml`/`boxblur.xml`, MLT avfilter.gblur | Reuse + GPU native | P0 | ☐ |
| Reduce Interlace Flicker | Soften horizontal lines | ✘ | Implement (small vertical blur on interlaced) | P3 | ☐ |
| Sharpen | Sharpness amount | ✔ (avfilter unsharp via MLT) | Reuse + native | P1 | ☐ |
| Unsharp Mask | Amount/radius/threshold | ✔ (avfilter unsharp) | Reuse + native | P1 | ☐ |

### B3. Channel (`channel-effects.html`)
| Effect | Documented | Kdenlive | Plan | Pri | Status |
|---|---|---|---|---|---|
| Invert | Inverts RGB/HLS/channel/alpha; "Blend With Original" 0–100% | ✔ `invert.xml` | Reuse; extend channels+blend param | P1 | ☐ |

### B4. Color Correction (`color-correction-effects.html`, `available-color-correction-effects.html`)
| Effect | Documented | Kdenlive | Plan | Pri | Status |
|---|---|---|---|---|---|
| ASC CDL | Slope/offset/power + saturation | **✔ audit correction:** `frei0r.sopsat` (SOP/Sat) — its XML description literally states "according to the ASC CDL (Color Decision List)"; Slope/Offset/Power per RGBA channel + overall Saturation, all keyframeable. (Earlier note that no CDL service exists was wrong: `avfilter.cdl` doesn't exist, but frei0r.sopsat does) | Reuse; consider native GPU CDL in the engine later | P1 | ☐ |
| Brightness & Contrast | ± brightness/contrast | ✔ `brightness.xml` | Reuse | P0 | ☐ |
| Lumetri Color | Full grading panel: Basic Correction (temp/tint/exposure/contrast/highlights/shadows/whites/blacks/saturation), Creative (looks), Curves, Color Wheels, HSL Secondary, Vignette | Partial — `lift_gamma_gain`, `hsl_primaries`, `hsl_range`, `luma`, LUT support | Implement FrameForge Grading panel assembling existing + new tools (§E) | P0 | ☐ |
| Tint | Map black/white to colors | ✔ `sepia`/`gradientmap` similar | Reuse/extend | P1 | ☐ |
| Video Limiter | Clamp to broadcast-legal signal | ✔ `avfilter.limiter` (audit correction — XML exists; min/max/planes params) | Reuse | P2 | ☐ |

### B5. Distort (`distort-effects.html`)
| Effect | Documented | Kdenlive | Plan | Pri | Status |
|---|---|---|---|---|---|
| Corner Pin | 4-corner pin ("distorts an image by shifting its four corners… stretch, shrink, skew, twist, or mimic perspective" — docs verified) | Was ✘ (MLT's bare avfilter.perspective auto-exposed with unusable expression-GUI, on excluded list) | **Implemented (M0): curated `data/effects/avfilter/avfilter_perspective.xml` (Corner Pin, 8 pixel params + interpolation/sense), removed from excluded_effects.txt, categorized under Transform/Distort/Perspective — UNVERIFIED (no build available yet)** | P1 | ◐ Implemented, unverified |
| Lens Distortion | Curvature | ✔ `avfilter.lenscorrection` + frei0r defish0r (audit correction — XML exists) | Reuse | P2 | ☐ |
| Magnify | **Adobe (verified, archived docs):** "lets you enlarge all or part of an image, like using a magnifying glass, or scale the whole image beyond 100% without losing quality." | ✘ (no MLT/FFmpeg service) | **Native FrameForge DSP implemented (M0, slice 10): `FrameForge::Video::Magnify` (`src/frameforge/video/magnify.*`) — circular region inverse-mapped at 100/magnification toward the center, params center_x/center_y/radius/magnification, bilinear over a packed copy; v1 hard circle edge (documented simplification); Catch2 tests (2× sampling anchors, outside-circle untouched, 100% identity) — UNVERIFIED until M1 | P3 | ◐ DSP implemented, untested |
| Mirror | Mirror horizontally/vertically | ✔ `mirror.xml` | Reuse | P1 | ☐ |
| Offset | Shift image, wrap-around | ✔ (affine) | Reuse/implement | P2 | ☐ |
| Spherize | **Adobe (verified, archived docs):** "distorts a layer by wrapping a region of the image onto a sphere." | ✘ (no MLT/FFmpeg service) | **Native FrameForge DSP implemented (M0, slice 9): `FrameForge::Video::Spherize` (`src/frameforge/video/spherize.*`) — inverse-mapped bulge (offset / (1 + strength·(1−d²)), seamless at the radius, center magnification), params amount/radius_percent, bilinear sampling over a packed frame copy (shared `sample.h`); Adobe's exact internal curve is undocumented — this bulge model is the recorded v1 interpretation; Catch2 tests (gray edge-blend anchor at d→0, boundary identity, outside-radius untouched, alpha preserved) — UNVERIFIED until M1 | P3 | ◐ DSP implemented, untested |
| Turbulent Displace | Fractal-based displacement | ✘ (frei0r distortion options) | Implement | P3 | ☐ |
| Twirl | **Adobe (verified, archived docs):** "distorts an image by rotating a clip around its center. The image is distorted more sharply in its center than at the edges, causing a whirlpool result at extreme settings." | ✘ (no MLT/FFmpeg service) | **Native FrameForge DSP implemented (M0, slice 9): `FrameForge::Video::Twirl` (`src/frameforge/video/twirl.*`) — inverse mapping with angle·(1−d/radius) falloff (the documented center-dominant distortion), params angle/radius, frame center (documented v1 simplification), bilinear sampling over a packed copy; Catch2 tests (180° near-center swap anchors: dest(cx+2,cy) samples src(cx−2,cy), outside-radius identity, zero-angle identity) — UNVERIFIED until M1 | P3 | ◐ DSP implemented, untested |
| Wave Warp | Wave along axis | ✔ `wave.xml` | Reuse | P2 | ☐ |
| Rolling Shutter Repair | Reduce rolling-shutter artifacts | ✘ | Implement (P3) | P3 | ☐ |
| Warp Stabilizer | Full param set verified: Result (Smooth/No Motion), Smoothness, Method (Position / Position+Scale+Rotation / Perspective / Subspace Warp), Preserve Scale, Framing (Stabilize Only / Crop / Crop+Auto-scale / Synthesize Edges), Auto-scale (Maximum Scale, Action-Safe Margin), Additional Scale, Detailed Analysis, Fast Analysis, Rolling Shutter Ripple, Crop Less↔Smooth More, Synthesis Input Range, Synthesis Edge Feather/Cropping, Hide Warning Banner | ✔ `vidstab.xml` (MLT vidstab — position/similarity/translation models) | Reuse vidstab; implement native stabilizer with Premiere-equivalent method ladder later | P2 | ☐ |

### B6. Generate (`generate-effects.html`)
| Effect | Documented | Kdenlive | Plan | Pri | Status |
|---|---|---|---|---|---|
| 4-Color Gradient | **Adobe (verified, archived docs):** "Creates a gradient with four colors. Use the Positions and Colors controls to animate the positions and colors of the four effect points, blending them into solid-color circles, each with an effect point as its center." | ✘ (frei0r cairogradient covers 2-color gradients only) | **Native FrameForge DSP implemented (M0, slice 11): `FrameForge::Video::FourColorGradient` (`src/frameforge/video/fourcolorgradient.*`) — normalized inverse-square blend of the 4 effect-point colors (the closest point dominates = the documented solid-color circles); 12 params (4 positions ×2 + 4 packed colors); full-frame generator-style output; Catch2 tests (exact corner colors, hand-computed blend anchor (146,122,48) at (3,3), clamping) — UNVERIFIED until M1 | P2 | ◐ DSP implemented, untested |
| Lens Flare | **Adobe (verified, archived docs):** "Simulates what happens when bright light hits a camera lens." | ✘ (no usable MLT/FFmpeg service; frei0r has no equivalent) | **Native FrameForge DSP implemented (M0, slice 12): `FrameForge::Video::LensFlare` (`src/frameforge/video/lensflare.*`) — additive synthetic flare: quadratic core glow, halo ring at 60% of size, ghost mirrored through the frame center (three-element model documented as the v1 simplification); params center_x/center_y/brightness/size; alpha passes through; Catch2 tests (core/ring/ghost anchors hand-computed, far-pixel zero, zero-brightness identity) — UNVERIFIED until M1 | P3 | ◐ DSP implemented, untested |
| Lightning | **Adobe (verified, archived docs):** "Creates animated lightning bolts, Jacob's Ladders, or other electric visuals between two specified points in a clip. The effect animates automatically across the clip's duration without keyframes." | ✘ (no MLT/FFmpeg service) | **Native FrameForge DSP implemented (M0, slice 12): `FrameForge::Video::Lightning` (`src/frameforge/video/lightning.*`) — hash-noise-displaced polyline (tapered to zero at the pinned endpoints, per the documented two-point semantics) drawn additively via a DDA walker; 7 params (endpoints, amplitude, segments, seed — the seed is the evolution knob for the documented automatic animation until engine time is wired at M3); Catch2 tests (pinned endpoints, connected-path coverage lower bound, provable outside-axis untouched, clamping) — UNVERIFIED until M1 | P3 | ◐ DSP implemented, untested |
| Ramp | Linear/radial gradient with Ramp Scatter | Partial: `frei0r_cairogradient` (gradient generator) covers simple linear gradients | Reuse + native gradient with scatter later | P2 | ☐ |

### B7. Image Control (`image-control-effects.html`)
| Effect | Documented | Kdenlive | Plan | Pri | Status |
|---|---|---|---|---|---|
| Black & White | Grayscale, non-animatable | ✔ `greyscale.xml` | Reuse | P0 | ☐ |
| Color Pass | Grayscale except chosen color | ✔ `chroma_hold.xml` | Reuse | P1 | ☐ |
| Color Replace | Replace color, keep gray levels | ✔ (chroma) | Reuse/extend | P1 | ☐ |
| Gamma Correction | Midtone brightness, 1–28 (default 10) | ✔ `gamma.xml` | Reuse | P0 | ☐ |

### B8. Immersive/VR (`immersive-video-effects.html`) — 11 effects verified
VR Blur, VR Chromatic Aberrations, VR Color Gradients, VR De-Noise, VR Digital Glitch, VR Fractal Noise, VR Glow, VR Plane to Sphere, VR Projection, VR Rotate Sphere, VR Sharpen. VR transitions listed in §C.
Kdenlive: ✘. FrameForge: **defer entire VR category (P3)** — niche; requires 360-aware compositing.

### B9. Keying (`keying-effects.html`, `ultra-key-effect-parameters.html`)
| Effect | Documented | Kdenlive | Plan | Pri | Status |
|---|---|---|---|---|---|
| Alpha Adjust | Alpha/opacity ops | ✔ qtblend | Reuse | P0 | ☐ |
| Color Key | Simple chroma key | ✔ `chroma.xml` | Reuse | P0 | ☐ |
| Luma Key | Luminance key | ✔ `lumakey.xml` | Reuse | P0 | ☐ |
| Track Matte Key | Use matte track | ✔ `matte.xml` transition | Reuse | P1 | ☐ |
| Ultra Key | Advanced chroma keyer (settings page archived: Matte Generation/Generation/spill/etc.) | Partial (MLT `rotoscoping`/frei0r chroma) | Implement advanced chroma keyer (GPU) | P1 | ☐ |

### B10. Noise & Grain (`noise-and-grain-effects.html`)
| Effect | Documented | Kdenlive | Plan | Pri | Status |
|---|---|---|---|---|---|
| Noise | Add random pixels (amount of color/noise) | ✔ `grain.xml`, `dust.xml`, `oldfilm.xml` family | Reuse + extend | P2 | ☐ |

### B11. Perspective (`perspective-effects.html`)
| Effect | Documented | Kdenlive | Plan | Pri | Status |
|---|---|---|---|---|---|
| Basic 3D | Rotate around axes, swivel/tilt/distance, specular highlight | ✔ `rotate-3-way.xml` (partial) | Extend with swivel/distance/highlight | P2 | ☐ |
| Drop Shadow | Alpha-based shadow, can exceed clip bounds, Shadow Only | ✔ `dropshadow.xml` | Reuse | P1 | ☐ |

### B12. Stylize (`stylize-effects.html`)
| Effect | Documented | Kdenlive | Plan | Pri | Status |
|---|---|---|---|---|---|
| Alpha Glow | **Adobe (verified, archived docs):** "applies color along the edges of a masked alpha channel. You can make the color fade out or gradually shift to a second color as it extends from the edge." | ✘ (frei0r edgeglow/movit.glow are luminance-based, not alpha-based) | **Native FrameForge DSP implemented (M0, slice 8): `FrameForge::Video::AlphaGlow` (`src/frameforge/video/alphaglow.*`) — glow mask = blurred alpha − original alpha (shared box-blur utility), color_a→color_b gradient as it extends from the edge, glow composited under the original via straight-alpha over-composite (interior untouched); params glow/brightness/color_a/color_b (packed 0xRRGGBB); Catch2 tests (falloff anchors from prefix-sum math, white-glow exactness, color-shift direction, clamping) — UNVERIFIED until M1 | P2 | ◐ DSP implemented, untested |
| Brush Strokes | Paint-style | ✘ (frei0r has cartoon/paint) | Reuse frei0r | P3 | ☐ |
| Color Emboss | Emboss keeping color | Partial: `frei0r_emboss` (Emboss — grayscale result, color not preserved) | Extend frei0r wrapper or defer to native | P3 | ☐ |
| Find Edges | Edge detect | **✔ audit correction:** `avfilter_edgedetect` (Edge detection) + sobel/scharr/prewitt/kirsch/roberts family | Reuse | P2 | ☐ |
| Mosaic | Pixelate (h/v blocks, sharp) | ✔ `obscure.xml` | Reuse | P1 | ☐ |
| Posterize | Levels per channel | ✔ `threshold`/frei0r posterize | Reuse | P2 | ☐ |
| Replicate | **Adobe (verified, archived docs):** "divides the screen into tiles and displays the whole image in each tile. Adjust the number of tiles in each column and row by moving the slider." | ✘ (no MLT/FFmpeg service) | **Native FrameForge DSP implemented (M0, slice 10): `FrameForge::Video::Replicate` (`src/frameforge/video/replicate.*`) — grid tiling with modulo cells (partial last cells handled), each tile a bilinear copy of the whole frame over a packed source copy; params horizontal/vertical (1-16); Catch2 tests (2×2 anchor map on a distinct-value 4×4 frame, alpha replicated, 1×1 identity, clamping) — UNVERIFIED until M1 | P3 | ◐ DSP implemented, untested |
| Roughen Edges | **Adobe (verified, archived docs):** "roughs up the edges of a clip's alpha channel by using calculations. It gives rasterized text or graphics a naturally rough look, like that of eroded metal or typewriter text." | ✘ (no MLT/FFmpeg service) | **Native FrameForge DSP implemented (M0, slice 11): `FrameForge::Video::RoughenEdges` (`src/frameforge/video/roughenedges.*`) — alpha sampled at positions displaced by two channels of deterministic uint32-hash value noise (no global RNG state, no platform drift); params border/scale/evolution; RGB untouched; Catch2 tests (provable interior/corner invariants, identity, RGB pass-through; the edge-ring assertion is deterministic-once-built and flagged for possible threshold loosening at M1) — UNVERIFIED until M1 | P3 | ◐ DSP implemented, untested |
| Strobe Light | Periodic frames | ✔ `strobe.xml` | Reuse | P2 | ☐ |

### B13. Time (`time-effects.html`)
| Effect | Documented | Kdenlive | Plan | Pri | Status |
|---|---|---|---|---|---|
| Echo | Combine previous/next frames | **✔ audit correction:** `avfilter_tmix` (Temporal frames Mixer) approximates frame blending | Reuse (approximation) | P2 | ☐ |
| Posterize Time | New frame rate | ✔ `timer.xml` (frame rate) | Reuse | P2 | ☐ |

### B14. Transform (`transform-effects.html`)
| Effect | Documented | Kdenlive | Plan | Pri | Status |
|---|---|---|---|---|---|
| Auto Reframe | AI reframe for aspect ratios | ✘ | Implement (motion-tracking crop) | P2 | ☐ |
| Crop | Left/top/right/bottom %, feather | ✔ `crop.xml`, `qtcrop.xml` | Reuse | P0 | ☐ |
| Edge Feather | **Adobe (verified, archived docs):** "adds a soft black border around your video, giving it a subtle vignette look. You can adjust the Amount value to control the width of the border." | Partial approximation exists today: Planes Blur (`avfilter_boxblur`) exposes Alpha Radius | **Native FrameForge DSP implemented (M0, slice 8): `FrameForge::Video::EdgeFeather` (`src/frameforge/video/edgefeather.*`) — alpha-channel feather via a separable prefix-sum box blur (shared utility `src/frameforge/video/alphablur.*`), amount→radius=ceil(amount/2), scratch sized in prepare() (allocation-free render path), RGB untouched; Catch2 tests (bypass, horizontal ramp + monotonicity, vertical pass) — UNVERIFIED until M1 | P2 | ◐ DSP implemented, untested |
| Horizontal/Vertical Flip | Mirror | ✔ (affine/rotation_keyframable) | Reuse | P0 | ☐ |
| Transform | Scale/skew/position/rotation/opacity + shutter angle | ✔ `rotation_keyframable.xml`, affine | Reuse; add shutter-angle motion blur later | P0 | ☐ |

### B15. Transition effects (`transition-effects.html`) — effect-style transitions
Block Dissolve, Gradient Wipe, Linear Wipe — Kdenlive ✔ equivalents (`wipe`, luma-based transitions); plan: Reuse (P1).

### B16. Utility (`utility-effects.html`)
Cineon Converter — **Adobe (verified, archived docs):** "high degree of control over the color conversions of Cineon frames… The 10 bits of data available in each Cineon channel…" · **Native FrameForge DSP implemented (M0, slice 9): `FrameForge::Video::CineonConverter` (`src/frameforge/video/cineon.*`) — canonical Cineon 10-bit log curve (ref white 685, 18% gray ≈ code 470 → 0.192, toe 0.0108 at black 95; 300 codes/decade — anchors cross-checked), params conversion/black_point/white_point/gamma, per-channel LUT; Catch2 tests (gray/white/toe anchors, Linear→Log round-trip) — Kdenlive ✘ XML-side; UNVERIFIED until M1.

### B17. "Modern effects" (Premiere 26.x additions, `types-of-effects/effects.html`)
New browsable groups verified: **Tools FX, Lights and Glows FX, Distort FX, Essentials FX, Motion**, plus documented **Gradient effect, Channel Blur, Noise effect, "Surprise Me" button**. These overlap the classic inventory above. Kdenlive ✘ for the grouped browsing UX. FrameForge: effect browser categories modeled on this UX (original design) — P2.

### B18. Obsolete/removed (removed from Premiere; `commonly-used-effects/effects-and-transitions-removed-from-adobe-premiere.html` + `effects-and-transitions-reorganization.html`)
58 removed effects (incl. Arithmetic, Auto Color/Contrast/Levels, Bevel Alpha/Edges, Blend, Calculations, Camera View, Cell Pattern, Change Color, Channel Mixer, Compound Blur, Convolution Kernel, Dust & Scratches, Emboss, Equalize, Fast Blur, Fast Color Corrector, Ghosting, Grid, Image Matte Key, Leave Color, Luma Corrector/Curves, Median, Noise Alpha/HLS/HLS Auto, Non Red Key, Paint Bucket, Radial Shadow, Remove Matte, RGB Color Corrector/Curves, Set Matte, Shadow/Highlight, Solarize, Solid Composite, Texturize, Three-Way, Threshold, Timecode, Clip Name, Write-On…) and 17 removed transitions — each with Adobe's documented replacement. **FrameForge will NOT recreate removed effects**; parity targets the *current* inventory above. Full table archived in `research/adobe/extracted-tables.md`.

### B19. Keyframes & interpolation (core to all effects)
Documented: add/select/copy/snap/delete keyframes, Bezier interpolation, value graphs (`control-effects-and-transitions-using-keyframes/` tree). Kdenlive ✔ full keyframe model (`src/assets/keyframes/`, KeyframeModel). Plan: Reuse (P0). Interpolation parity: verify Kdenlive supports linear + Bezier (spline) — extend to full Bezier handles if missing.

---

## C. Video transitions (complete documented inventory)

Source: `list-of-video-transitions.html` + `list-of-video-dissolve-transitions.html` (verified 2026-10-12).

| Category | Documented transitions | Kdenlive today | Plan | Pri | Status |
|---|---|---|---|---|---|
| Dissolve | Additive Dissolve, Cross Dissolve, Dip To Black, Dip To White, Film Dissolve (linear-light blend), Morph Cut (face-tracking interpolation), Non-Additive Dissolve | ✔ `dissolve.xml`, `mix.xml`, `luma.xml`, fade in/out; ✘ Morph Cut, Film Dissolve specifics | Reuse; implement Morph Cut later (P3); Film Dissolve in linear light (P1) | P0/P3 | ☐ |
| Iris | Iris Box, Iris Cross, Iris Diamond, Iris Round | ✔ via luma wipes (`data/lumas/`) | Reuse luma images | P1 | ☐ |
| Page Peel | Page Peel, Page Turn | Partial (luma-based approximations) | Implement proper geometric page turn (P3) | P3 | ☐ |
| Slide | Band Slide, Center Split, Push, Slide, Split, Whip | ✔ `slide.xml`, `affine.xml` | Reuse; add missing variants | P1 | ☐ |
| Wipe | Band Wipe, Barn Doors, Checker Wipe, CheckerBoard, Clock Wipe, Inset, Paint Splatter, Pinwheel, Radial Wipe, Random Blocks, Random Wipe, Spiral Boxes, Venetian Blinds, Wedge Wipe, Wipe, Zig-Zag Blocks | ✔ `wipe.xml` + 18 shipped lumas | **5 missing wipes added (M0): `band_wipe`, `barn_doors`, `random_blocks`, `wedge`, `zigzag_blocks` luma files (HD+PAL, deterministically generated by `research/tools/generate_missing_wipes.ps1`, verified P5 structure/range), auto-discovered by the luma transition's `%lumaPaths` — UNVERIFIED in app.** Remaining gaps: Inset (≈square.pgm), Paint Splatter, Random Wipe (deferred) | P1 | ◐ Partial, unverified |
| Zoom | Cross Zoom | ✘ | Implement | P2 | ☐ |
| Immersive Video | VR Chroma Leaks, VR Gradient Wipe, VR Iris Wipe, VR Light Leaks, VR Light Rays, VR Mobius Zoom, VR Random Blocks, VR Spherical Blur | ✘ | Defer (P3) | P3 | ☐ |
| Audio crossfades | Verified from official page (archived `audio-crossfade-transitions.md`): **Constant Gain** (constant rate in/out, can dip in volume at midpoint), **Constant Power** (smooth dissolve-like fade, slow→fast/fast→slow), **Exponential Fade** (logarithmic curves) | ✔ `mix` transition — but linear curve only: MLT's `mix_audio` has a hidden equal-power path (`power = start < -1.0`, verified in upstream `transition_mix.c`), while Kdenlive hardcodes `start=-1` (trackmodel.cpp) and mix.xml exposed no curve choice | **Implemented (M0, mix.xml): "Crossfade Curve" parameter exposing Constant Gain (-1, preserved default) and Constant Power (-2, MLT's sin/cos equal-power path). Pure-XML change — verified in Kdenlive source that creation (start=-1 init), UI updates (AssetParameterModel→`m_asset->set`), project load (loadMix copies actual property values into `value` attributes) and render-copy all flow correctly. Exponential Fade (investigated, slice 13): FFmpeg `avfilter.afade` supports the documented logarithmic curve (`curve=log`, verified against local FFmpeg 8.1.2) and is upstream-excluded only under "unusable with a simple GUI" (excluded_effects.txt §39 — rescuable like Corner Pin) — but shipping an XML is deferred: the MLT integration path rests on three unverified mechanisms (clip-relative timing of avfilter audio params, duration-unit passing through MLT's avfilter module, and the `totime` XSD attribute has no shipped usage — timer.xml keeps its totime examples commented out). Verify at the M1 build, then ship. UNVERIFIED (no build)** | P0 | ◐ Implemented, unverified |

Transition behavior documented in "Apply video transitions": default transition, single-sided, alignment (Center at Cut/Start at Cut/End at Cut), copy/paste, replace, clip handles. Kdenlive ✔ equivalents via composition model (CompositionModel). Plan: Reuse (P0).

---

## D. Audio effects (complete documented inventory)

Source: `apply-audio-effects/audio-effects-library.html` (verified 2026-10-12; page fully archived).

| Category | Documented effects | Kdenlive today (via MLT: ladspa/sox/frei0r + `volume.xml` etc.) | Plan | Pri | Status |
|---|---|---|---|---|---|
| Amplitude & Compression | Channel Mixer, Channel Volume, DeEsser, Dynamics, Dynamics Processing, Multiband Compressor, Single-band Compressor, Tube-modeled Compressor | Partial (sox/ladspa dynamics, `dynamic_loudness.xml`, `audiobalance.xml`, `channelcopy.xml`) | Implement native DSP chain for core: compressor, limiter, gain (P0); DeEsser (P2) | P0 | ☐ |
| Delay & Echo | Analog Delay, Delay, Multitap Delay | ✔ ladspa/sox delay variants | Reuse + curated presets | P2 | ☐ |
| Filter & EQ | Bandpass, Bass, FFT Filter, Graphic EQ (10/20/30 band), Highpass, Lowpass, Notch, Parametric EQ, Scientific Filter, Treble | ✔ ladspa EQs | Implement native Parametric EQ (P1); reuse rest | P1 | ☐ |
| Modulation | Chorus/Flanger, Flanger, Phaser | ✔ ladspa | Reuse | P2 | ☐ |
| Noise Reduction/Restoration | Automatic Click Remover, DeHummer, DeNoise, DeReverb | Partial: RNNoise suppression via `ladspa_librnnoise`; `bandreject` = single manual notch; DeEsser exists | **Blocked analysis (2026-10-12):** upstream flags `avfilter.afftdn`, `avfilter.adeclick`, `avfilter.adeclip`, `avfilter.anlmdn` under "NOT WORKING AUDIO AVIFILTER" (`data/excluded_effects.txt`) — windowed-DSP audio avfilters broken through MLT (re-test at M1 build); DeNoise/Automatic Click Remover remain blocked there. **DeHummer: native FrameForge DSP implemented (M0, slice 6)** — `FrameForge::Audio::DeHummer` (`src/frameforge/audio/`) implements the full Adobe-documented parameter set (frequency, Q, gain, harmonics, harmonic slope, output-hum-only) as a time-domain biquad notch cascade behind the first FrameForge engine interface `IEffect` (`src/frameforge/interfaces/ieffect.h`), pure C++ with Catch2 DSP tests (`tests/frameforge_dsptest.cpp`); UNVERIFIED until the M1 build, playback wiring lands with the engine (M3) | P1 | ◐ DeHummer DSP implemented, untested |
| Reverb | Convolution Reverb, Studio Reverb, Surround Reverb | ✔ ladspa/sox reverb | Reuse; Convolution Reverb later (P2) | P1 | ☐ |
| Special | Distortion, Fill Left with Right, Fill Right with Left, Guitar Suite, Invert, Loudness Radar, Mastering, Swap Channels, Vocal Enhancer | Partial (`swapchannels.xml`, `mono.xml`, `audiomap`) | Implement Loudness Radar (P2); reuse rest | P2 | ☐ |
| Stereo Imagery | Stereo Expander | ✘ | Implement (P3) | P3 | ☐ |
| Time & Pitch | Pitch Shifter | ✔ `rbpitchscale.xml` | Reuse | P2 | ☐ |
| Obsolete audio | Page lists obsolete set | — | Do not target | — | ☐ |

Mixer/automation documented: Audio Track Mixer, submixes, channel mapping, automation modes (Read/Touch/Latch/Write/Off), pre/post-fade effects, panning/balancing, Loudness Meter (RMS/peak/LUFS). Kdenlive ✔ MixerManager (`src/audiomixer/`), keyframed volume; automation latching partial. Plan: Reuse + implement full automation modes (P1), Loudness Meter LUFS (P2).

---

## E. Color correction & color management

Sources: `correct-color/` tree (verified 2026-10-12).

| Feature | Documented | Kdenlive | Plan | Pri | Status |
|---|---|---|---|---|---|
| Basic correction sliders | White balance (temp/tint), Exposure, Contrast, Highlights, Shadows, Whites, Blacks, Saturation | ✔ `lift_gamma_gain` + LADSPA-style; partial slider set | Implement full Basic Correction panel natively | P0 | ☐ |
| Color wheels & Match | Color Wheels (shadows/midtones/highlights Lumetri), "Match color between shots" | ✘ wheels; ✔ similar via lift/gamma/gain per-range `hsl_range` | Implement 3-wheel UI on existing per-range math | P1 | ☐ |
| RGB curves | "Correct color using RGB curves" | **✔ audit correction:** `avfilter_curves` with dedicated `av_curve` interactive curve widget (All/R/G/B channels) + `frei0r_bezier_curves` | Reuse; verify widget at M1 build | P1 | ☐ |
| HSL Secondary | "Correct color using HSL Secondary Controls" | ✔ `hsl_primaries.xml`, `hsl_range.xml` | Reuse + UI | P1 | ☐ |
| LUTs | Install/apply/save-export LUTs (.cube etc.) | ✔ LUT support incl. `data/luts/` | Reuse; export LUT later | P1 | ☐ |
| Scopes | Lumetri Scopes (verified page exists) | ✔ `src/scopes/` (vectorscope, waveform, RGB parade) | Reuse | P1 | ☐ |
| Color Management | "How Color Management works", project/sequence/clip color spaces, output color space, tone mapping | Partial (MLT colorspace conversion) | Implement OCIO-style pipeline (P2) | P2 | ☐ |
| Vignette | "Create vignettes using Lumetri Color / masks" | ✔ `vignette.xml` | Reuse | P1 | ☐ |
| Color mode (beta) | New Color page with Clip Grid, Adjust/Style operations, grade management | ✘ | Defer; watch stabilization | P3 | ☐ |
| Auto Color | Removed per §B18; Lumetri auto options current | — | Implement simple auto-levels (P3) | P3 | ☐ |

---

## F. Titles & graphics

Source: `add-text-images/` tree (verified 2026-10-12).

| Feature | Documented | Kdenlive | Plan | Pri | Status |
|---|---|---|---|---|---|
| Titles | "Create titles" via Properties panel/graphics | ✔ Titler (`src/titler/`, QML) | Reuse; rebrand/polish | P0 | ☐ |
| Text styles | Create/apply text styles, Linked & Track Styles, color fonts, emojis | Partial | Implement style library (P2) | P2 | ☐ |
| Shapes & pen | Rectangle/Ellipse/4-point polygon/Pen/freeform, masks | ✔ shape tools + rotoscoping | Reuse | P1 | ☐ |
| Layers & animation | Group layers, clip layers, credit rolls, gradients, animate via Effect Controls/Properties | ✔ (QML graphics with keyframes) | Reuse; add gradient fills (P2) | P1 | ☐ |
| Responsive design | "Create responsive graphics", preserve intro/outro animations | ✘ | Defer (P3) | P3 | ☐ |
| Motion Graphics templates (.mogrt) | Install/customize/export templates | ✘ (Kdenlive has its own QML template system) | Support Kdenlive QML templates; .mogrt NOT target (Adobe format) | P2 | ☐ |
| Captions | Captions overview, Speech-to-Text transcription, caption styles, translate, single-word captions | ✔ SubtitleModel + SubtitleEdit | Reuse; local STT optional | P1 | ☐ |

---

## G. AI-assisted features (documented, with cloud/local classification)

| Feature | Documented behavior | Cloud dependency (per Adobe docs) | Kdenlive | Plan | Pri | Status |
|---|---|---|---|---|---|---|
| Speech to Text | Transcribe to captions | Language packs download; on-device processing documented for Premiere | ✔ (VOSK/whisper local) | Reuse local STT (no cloud) | P1 | ☐ |
| Text-Based Editing | Edit sequence from transcript; delete pauses; remove speaker | Same as STT | ✔ TextBasedEdit | Reuse | P1 | ☐ |
| Enhance Speech | One-click voice cleanup | AI processing (local render documented w/ GPU requirements page) | ✘ | Optional plugin later; disclose requirements | P3 | ☐ |
| Auto Reframe | AI reframe sequences/clips | Local analysis | ✘ | Implement motion-based reframe (P2) | P2 | ☐ |
| Scene Edit Detection | Detect original cuts | Local analysis | ✔ `SceneSplitTask` (FFmpeg-8.1.2-compat verified 2026-10-12) | Reuse | P2 | ✔ |
| Media Intelligence / Search panel | Semantic visual/audio/text search, tags, .prin analysis files, background analysis | Local analysis documented (FAQ verified) | ✘ | Defer (P3); consider clip tagging via local embeddings | P3 | ☐ |
| Generative Extend | Extend clip by generated frames | **Cloud (Firefly)** | ✘ | NOT planned (no cloud dependency in FrameForge) | — | ☐ |
| Generative Media Tool / Generate Music | Generate media/music | **Cloud (Firefly)** | ✘ | NOT planned | — | ☐ |
| Morph Cut | Face-tracking jump-cut smoothing | Local analysis | ✘ | Research later (P3) | P3 | ☐ |
| Remix (audio) | Retarget music duration | Local analysis | ✘ | P3 | P3 | ☐ |
| Object masking | Track & mask objects | Local analysis (ML) | ✘ (rotoscoping exists) | Extend rotoscoping w/ tracker (P2) | P2 | ☐ |
| Auto duck audio | Duck music under dialogue | Local | ✔ (`autoduck.xml` transition) | Reuse | P1 | ☐ |
| Premiere AI Assistant (beta) | Conversational assistant | **Cloud** | ✘ | NOT planned | — | ☐ |

FrameForge AI policy: only local, optional, provider-independent AI (STT, scene detection, reframe); nothing phones home. No model downloads without explicit user approval.

---

## H. Export & interoperability

Sources: `render-and-export/` tree (verified 2026-10-12).

| Feature | Documented | Kdenlive | Plan | Pri | Status |
|---|---|---|---|---|---|
| Export video/audio | Export settings UI: Basic Video Settings (Frame Size, Frame Rate, Field Order, Aspect, Render at Maximum Depth, Use Maximum Render Quality, Time Interpolation options), Encoding settings (Performance software/GPU, Profile, Level, Export Color Space, HDR10 metadata, Color Primaries, Luminance min/max), Bitrate settings (CBR/VBR 1/2-pass), Multiplexer, Audio format/basic/bitrate settings, Captions, Metadata export, Set Start Timecode | ✔ RenderDialog + renderpresets via MLT `melt`/`ffmpeg` | Reuse MLT export; add Media Foundation HW encode later (P2) | P0 | ☐ |
| Formats | "Supported export file formats" (page archived; list is image-grid — verify at implementation) | ✔ via MLT/FFmpeg (H.264/H.265/ProRes/VP9/… per build) | Verify format-by-format at implementation | P0 | ☐ |
| Hardware encoding | "Enable Hardware Encoding" (Intel QSV/AMD/NVIDIA documented) | Partial (depends on ffmpeg build) | Implement MF H.264/H.265 HW encode (P2) | P2 | ☐ |
| Presets | Preset Manager, create/save/manage custom presets, Match Source | ✔ encodingprofiles/renderpresets | Reuse; add Preset Manager UI (P1) | P1 | ☐ |
| Queue-based render | Export via Adobe Media Encoder | ✔ render queue in Kdenlive (background render jobs) | Reuse job system | P1 | ☐ |
| Smart rendering | "Smart rendering supported formats" | Partial (proxy/preview reuse) | Preview-cache reuse (P2) | P2 | ☐ |
| Render & replace | Render section, replace with original | ✔ timeline preview chunks | Reuse | P2 | ☐ |
| Stills | Export still frame/image | ✔ | Reuse | P1 | ☐ |
| EDL | Export project as EDL | **Implemented (M0, slice 5): `EdlExport` (`src/export/edlexport.*`) — CMX3600 cuts-only export of the bottom video track ("V1"), menu entry "EDL Export…" next to the OTIO export, mix-adjusted in/out points (mirrors OTIO export behavior), source timecode in the media's original rate, `* FROM CLIP NAME:` / `* FROM FILE:` comments, Catch2 test (`tests/edltest.cpp`) registered in the test suite. UNVERIFIED — no build; every model API used was verified against existing usage in `otioexport.cpp`** | v2: dissolve events, audio tracks, drop-frame TC | P2 | ◐ Implemented, unverified |
| FCP XML | Export Final Cut Pro XML | ✔ (via OpenTimelineIO) | Reuse OTIO (P1) | P1 | ☐ |
| AAF | Export AAF (incl. audio for ProTools OMF) | ✘ | Implement via OTIO adapter if available (P2) | P2 | ☐ |
| OMF | Export OMF for Pro Tools | ✘ | P3 | P3 | ☐ |
| Interop import | FCP XML import, Avid MC project import (verified doc titles) | ✔ OTIO | Reuse | P1 | ☐ |

---

## Verification log

| Date | Action | Result |
|---|---|---|
| 2026-10-12 | Fetched helpx docs tree (`list-of-effects-and-transitions.html`) | OK (2,426 lines archived) |
| 2026-10-12 | Archived 49 official pages to `research/adobe/raw/` (r.jina.ai reader; direct helpx fetch returns 403) | OK; manifest written |
| 2026-10-12 | Extracted content headings (516) and tables (155 rows) | OK |
| 2026-10-12 | Attempted machine extraction of accelerated/32-bit/YUV lists | FAILED — pages render as image grids; marked unverified |
| 2026-10-12 | Audio crossfade transitions page | NOT FETCHED yet — gap |
| 2026-10-12 | Adobe Premiere websearch tool | FAILED ("cancelled") — used direct-URL fetching instead |
| 2026-10-12 | Full effect-name audit sweep: extracted all ~290 effect display names from `data/effects` XMLs and diffed against every ✘ claim in this matrix | 9 more false-gaps corrected (SceneSplitTask, ASC CDL via frei0r.sopsat, Directional Blur dblur, Find Edges edgedetect+sobel family, RGB-curves av_curve widget, Echo ≈ tmix, + approximations for Camera Blur/Color Emboss/Ramp); remaining ✘ rows are now confirmed genuine gaps |
| 2026-10-12 | SceneSplitTask FFmpeg-compatibility check: ran the task's exact command on tests/small.mkv against local FFmpeg 8.1.2 | Exit 0; `[Parsed_showinfo`/`pts_time:` output format matches the parser — feature verified compatible |
