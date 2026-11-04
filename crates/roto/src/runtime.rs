//! Hardware and execution-provider capability *reporting*.
//!
//! The finished application must run inference on the user's own hardware, and
//! must choose an execution provider that really supports the model. This module
//! is deliberately honest about the difference between what is known and what is
//! not:
//!
//! * CPU is reported as available, because the process is running on one.
//! * Every GPU provider is reported as **not probed** unless the app has actually
//!   learned otherwise from a runtime at run time. Nothing here loads a model,
//!   creates a device or queries a driver, so no claim about DirectML, CUDA,
//!   Vulkan or AMD/NVIDIA/Intel support is made — per the project's rules, those
//!   claims require verified operator and runtime compatibility first.
//! * RAM and GPU memory are reported as unknown, because reading them needs an OS
//!   API this crate does not add a dependency for; the app's settings carry the
//!   user's own budget instead of a guess.

use serde::{Deserialize, Serialize};

/// What is known about one execution provider.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderState {
    /// The provider exists in this build and needs no extra package.
    Available,
    /// The provider would have to come from a runtime the user installs; nothing
    /// has been probed, so nothing is claimed.
    NotProbed,
    /// The model's operators are known not to run on this provider.
    Unsupported,
}

impl ProviderState {
    pub fn label(self) -> &'static str {
        match self {
            ProviderState::Available => "available",
            ProviderState::NotProbed => "not probed",
            ProviderState::Unsupported => "unsupported for this model",
        }
    }
}

/// One provider's report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStatus {
    pub id: &'static str,
    pub label: &'static str,
    pub state: ProviderState,
    /// What the app should tell the user about it.
    pub note: &'static str,
    /// Whether a model can be planned on it right now.
    pub usable_for_inference: bool,
}

/// The machine's capabilities, as far as this build may claim to know them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeReport {
    pub os: &'static str,
    pub arch: &'static str,
    pub logical_cpus: usize,
    pub providers: Vec<ProviderStatus>,
    /// Peak memory the user's settings allow, in MiB, when they have chosen one.
    pub memory_budget_mb: Option<u64>,
}

impl RuntimeReport {
    /// Providers that could actually run a model today.
    pub fn usable_providers(&self) -> Vec<&ProviderStatus> {
        self.providers.iter().filter(|provider| provider.usable_for_inference).collect()
    }

    /// Whether any accelerator is known to work. Always false until a runtime
    /// says otherwise at run time.
    pub fn has_verified_accelerator(&self) -> bool {
        self.providers.iter().any(|provider| provider.usable_for_inference && provider.id != "cpu")
    }

    pub fn summary(&self) -> String {
        let usable = self.usable_providers().len();
        format!(
            "{} {} · {} logical CPUs · {usable} provider(s) usable, {} not probed",
            self.os,
            self.arch,
            self.logical_cpus,
            self.providers.iter().filter(|provider| provider.state == ProviderState::NotProbed).count()
        )
    }

    /// What the workspace tells the user before a job starts.
    pub fn guidance(&self) -> &'static str {
        if self.has_verified_accelerator() {
            "A hardware provider reported itself usable; FrameForge will still fall back to CPU if a model fails to load."
        } else {
            "No hardware provider has been verified on this machine, so inference would run on the CPU with the model you install. FrameForge never assumes a GPU backend works."
        }
    }
}

/// Report the machine, without probing any model.
pub fn report(memory_budget_mb: Option<u64>) -> RuntimeReport {
    let logical_cpus = std::thread::available_parallelism().map(|count| count.get()).unwrap_or(1);
    RuntimeReport {
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        logical_cpus,
        providers: vec![
            ProviderStatus {
                id: "cpu",
                label: "CPU",
                state: ProviderState::Available,
                note: "Always available; the measured XMem CPU baseline for this project is on the target laptop.",
                usable_for_inference: true,
            },
            ProviderStatus {
                id: "onnx-directml",
                label: "ONNX Runtime · DirectML",
                state: ProviderState::NotProbed,
                note: "Would need an ONNX Runtime build with the DirectML provider. Not probed, and no model is claimed to run on it.",
                usable_for_inference: false,
            },
            ProviderStatus {
                id: "onnx-cuda",
                label: "ONNX Runtime · CUDA",
                state: ProviderState::NotProbed,
                note: "Would need an ONNX Runtime GPU build and a matching driver. Not probed.",
                usable_for_inference: false,
            },
            ProviderStatus {
                id: "onnx-vulkan",
                label: "ONNX Runtime · Vulkan",
                state: ProviderState::NotProbed,
                note: "Not probed. Vulkan execution has operator gaps for these models; nothing is claimed until a runtime reports success.",
                usable_for_inference: false,
            },
            ProviderStatus {
                id: "python-torch-cpu",
                label: "Python · PyTorch (CPU)",
                state: ProviderState::NotProbed,
                note: "The path the existing XMem adapter uses. Absent until the user installs a runtime and consents.",
                usable_for_inference: false,
            },
        ],
        memory_budget_mb,
    }
}

/// The resolution policy: what working size to ask a model for, given the
/// machine and the source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolutionPolicy {
    pub max_width: u32,
    pub max_height: u32,
    /// Whether the caller may raise this when the user asks for quality.
    pub adaptive: bool,
}

/// The speed/fidelity knobs, as planning inputs.
///
/// Nothing here measures the machine: every number is either the user's choice
/// or arithmetic on it. `per_frame_mb` is an *estimate* and says so, because a
/// fabricated measurement is worse than an honest guess the user can override.
///
/// `frame_stride` is the one knob that changes what a tracker does: it propagates
/// every Nth frame and repeats the last mask in between. That is a real speed
/// trade, so the mask cache still holds every frame and the temporal QC pass sees
/// the repeated masks and can flag the stutter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerformanceSettings {
    pub max_width: u32,
    pub max_height: u32,
    /// Propagate every Nth frame (1 = every frame).
    pub frame_stride: u32,
    /// Planning budget for resident memory, in MiB.
    pub memory_budget_mb: u64,
    /// Keep inference off the UI thread. The run path always does; this records
    /// the preference for the queue.
    pub background: bool,
}

impl Default for PerformanceSettings {
    fn default() -> Self {
        Self { max_width: 480, max_height: 270, frame_stride: 1, memory_budget_mb: 2_048, background: true }
    }
}

impl PerformanceSettings {
    pub const MIN_SIDE: u32 = 64;
    pub const MAX_SIDE: u32 = 3_840;
    pub const MAX_STRIDE: u32 = 8;
    pub const MIN_BUDGET_MB: u64 = 512;
    pub const MAX_BUDGET_MB: u64 = 65_536;
    /// The largest working area offered, so "3840x2160" cannot quietly become a
    /// 32 MP tensor on integrated graphics.
    pub const MAX_AREA: u64 = 1_920 * 1_080;

    /// Clamp every field into a range this build will actually honour.
    pub fn sanitised(self) -> Self {
        let mut width = self.max_width.clamp(Self::MIN_SIDE, Self::MAX_SIDE);
        let mut height = self.max_height.clamp(Self::MIN_SIDE, Self::MAX_SIDE);
        let area = width as u64 * height as u64;
        if area > Self::MAX_AREA {
            let scale = (Self::MAX_AREA as f64 / area as f64).sqrt();
            width = ((width as f64 * scale) as u32).max(Self::MIN_SIDE);
            height = ((height as f64 * scale) as u32).max(Self::MIN_SIDE);
        }
        Self {
            max_width: width,
            max_height: height,
            frame_stride: self.frame_stride.clamp(1, Self::MAX_STRIDE),
            memory_budget_mb: self.memory_budget_mb.clamp(Self::MIN_BUDGET_MB, Self::MAX_BUDGET_MB),
            background: self.background,
        }
    }

    pub fn resolution(self) -> ResolutionPolicy {
        let clean = self.sanitised();
        ResolutionPolicy { max_width: clean.max_width, max_height: clean.max_height, adaptive: true }
    }

    pub fn working_size(self, source: (u32, u32)) -> (u32, u32) {
        self.resolution().fit(source.0, source.1)
    }

    /// A *planning estimate* of one frame's resident cost at the working size:
    /// the RGB float tensor, the mask logits and one more activation buffer,
    /// plus the checkpoint's own footprint and framework overhead. It is not a
    /// measurement, and the panel says so.
    pub fn per_frame_mb(self, source: (u32, u32)) -> u64 {
        let (width, height) = self.working_size(source);
        let pixels = u64::from(width) * u64::from(height);
        (pixels.saturating_mul(16) / (1024 * 1024)).saturating_add(192)
    }

    /// How many frames the budget could hold, for an adapter that batches. The
    /// per-frame workers do not batch; the panel shows this as a ceiling, not a
    /// promise.
    pub fn recommended_batch(self, source: (u32, u32)) -> u32 {
        self.resolution().batch_frames(Some(self.sanitised().memory_budget_mb), self.per_frame_mb(source))
    }

    /// The frames a stride would actually propagate. The last frame is always
    /// included, so the tail of a range is never left to a repeat.
    pub fn tracked_frames(self, first: i64, last: i64) -> Vec<i64> {
        if last < first {
            return Vec::new();
        }
        let stride = i64::from(self.sanitised().frame_stride);
        let mut frames = Vec::new();
        let mut frame = first;
        while frame <= last {
            frames.push(frame);
            frame += stride;
        }
        if frames.last().copied() != Some(last) {
            frames.push(last);
        }
        frames
    }

    /// One line for the panel.
    pub fn summary(self, source: (u32, u32)) -> String {
        let clean = self.sanitised();
        let (width, height) = clean.working_size(source);
        let stride = if clean.frame_stride == 1 { "every frame".to_string() } else { format!("every {}th frame", clean.frame_stride) };
        format!(
            "{width}x{height} · {stride} · budget {} MB · ~{} MB/frame (estimate) · batch ceiling {} · {}",
            clean.memory_budget_mb,
            clean.per_frame_mb(source),
            clean.recommended_batch(source),
            if clean.background { "background" } else { "foreground" }
        )
    }
}

impl Default for ResolutionPolicy {
    fn default() -> Self {
        Self { max_width: 480, max_height: 270, adaptive: true }
    }
}

impl ResolutionPolicy {
    /// Fit within the policy without upscaling, and never return a zero size.
    pub fn fit(&self, width: u32, height: u32) -> (u32, u32) {
        if width == 0 || height == 0 {
            return (self.max_width.max(1), self.max_height.max(1));
        }
        let scale = (self.max_width as f32 / width as f32).min(self.max_height as f32 / height as f32).min(1.0);
        let out = ((width as f32 * scale).round() as u32, (height as f32 * scale).round() as u32);
        (out.0.max(1), out.1.max(1))
    }

    /// The frames that fit one batch, given a memory budget and a per-frame cost
    /// estimate. Conservative on purpose: a batch that does not fit is worse
    /// than a slow one.
    pub fn batch_frames(&self, memory_budget_mb: Option<u64>, per_frame_mb: u64) -> u32 {
        let per_frame = per_frame_mb.max(1);
        let budget = memory_budget_mb.unwrap_or(2_048);
        let frames = budget / per_frame;
        frames.clamp(1, 64) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_only_claims_what_it_can_know() {
        let report = report(Some(4_096));
        assert!(report.logical_cpus >= 1);
        assert!(!report.os.is_empty() && !report.arch.is_empty());

        let cpu = report.providers.iter().find(|provider| provider.id == "cpu").expect("cpu provider");
        assert_eq!(cpu.state, ProviderState::Available);
        assert!(cpu.usable_for_inference);
        assert_eq!(report.usable_providers().len(), 1, "only the CPU may be claimed usable");

        // Rule 10: no accelerator is claimed anywhere.
        assert!(!report.has_verified_accelerator());
        for provider in report.providers.iter().filter(|provider| provider.id != "cpu") {
            assert_eq!(provider.state, ProviderState::NotProbed, "{} must not be claimed", provider.id);
            assert!(!provider.usable_for_inference);
            assert!(provider.note.contains("Not probed") || provider.note.contains("not probed") || provider.note.contains("Absent"));
        }
        assert!(report.summary().contains("not probed"));
        assert!(report.guidance().contains("never assumes a GPU backend works"));
        assert!(ProviderState::Unsupported.label().contains("unsupported"));
    }

    #[test]
    fn the_resolution_policy_fits_without_upscaling() {
        let policy = ResolutionPolicy::default();
        assert_eq!(policy.fit(1_920, 1_080), (480, 270));
        assert_eq!(policy.fit(240, 135), (240, 135), "a small source is not upscaled");
        assert_eq!(policy.fit(0, 0), (480, 270), "an unknown source falls back to the policy");
        assert_eq!(policy.fit(1_000, 100), (480, 48));
        assert!(policy.adaptive);
    }

    #[test]
    fn batching_is_conservative_and_bounded() {
        let policy = ResolutionPolicy::default();
        assert_eq!(policy.batch_frames(Some(2_048), 512), 4);
        assert_eq!(policy.batch_frames(Some(100), 512), 1, "a batch never drops below one frame");
        assert_eq!(policy.batch_frames(Some(1_000_000), 1), 64, "and never grows past the cap");
        assert_eq!(policy.batch_frames(None, 0), 64, "a zero cost estimate is treated as one");
    }

    #[test]
    fn performance_settings_clamp_into_the_ranges_the_engine_honours() {
        let defaults = PerformanceSettings::default();
        assert_eq!(defaults.sanitised(), defaults);

        let wild = PerformanceSettings { max_width: 0, max_height: 9_000, frame_stride: 99, memory_budget_mb: 1, background: false }.sanitised();
        assert_eq!(wild.max_width, PerformanceSettings::MIN_SIDE);
        assert!(wild.max_height <= PerformanceSettings::MAX_SIDE);
        assert_eq!(wild.frame_stride, PerformanceSettings::MAX_STRIDE);
        assert_eq!(wild.memory_budget_mb, PerformanceSettings::MIN_BUDGET_MB);
        assert!(!wild.background);

        let huge = PerformanceSettings { max_width: 3_840, max_height: 3_840, ..PerformanceSettings::default() }.sanitised();
        assert!(u64::from(huge.max_width) * u64::from(huge.max_height) <= PerformanceSettings::MAX_AREA);
    }

    #[test]
    fn a_stride_propagates_keyframes_and_never_drops_the_last_frame() {
        let settings = PerformanceSettings { frame_stride: 3, ..PerformanceSettings::default() };
        assert_eq!(settings.tracked_frames(0, 9), vec![0, 3, 6, 9]);
        assert_eq!(settings.tracked_frames(0, 10), vec![0, 3, 6, 9, 10], "the tail is always tracked");
        assert_eq!(settings.tracked_frames(5, 5), vec![5]);
        assert!(settings.tracked_frames(9, 5).is_empty());

        let every = PerformanceSettings::default();
        assert_eq!(every.tracked_frames(0, 3), vec![0, 1, 2, 3]);
    }

    #[test]
    fn the_cost_estimate_is_an_estimate_and_scales_with_the_working_size() {
        let small = PerformanceSettings { max_width: 320, max_height: 180, ..PerformanceSettings::default() };
        let large = PerformanceSettings { max_width: 1_280, max_height: 720, ..PerformanceSettings::default() };
        assert!(large.per_frame_mb((1_920, 1_080)) > small.per_frame_mb((1_920, 1_080)));
        assert!(small.per_frame_mb((1_920, 1_080)) > 0);
        assert_eq!(small.working_size((1_920, 1_080)), (320, 180));
        assert_eq!(small.working_size((100, 100)), (100, 100), "no upscaling");
        let batch = small.recommended_batch((1_920, 1_080));
        assert!((1u32..=64).contains(&batch), "{batch}");
        assert!(small.summary((1_920, 1_080)).contains("estimate"));
        assert!(PerformanceSettings { frame_stride: 4, ..PerformanceSettings::default() }.summary((1_920, 1_080)).contains("every 4th frame"));
    }
}
