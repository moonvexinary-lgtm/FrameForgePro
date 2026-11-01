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
}
