//! The accelerator seam. Inference is CPU-only today; this names the alternatives a backend may
//! report so a future optional crate (OpenVINO on Intel x86) can plug in without changing the
//! command or result shapes. CPU is always available, so the feature behaves identically there.

use std::fmt;

use serde::Serialize;

/// Where the model runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Accelerator {
    /// Pure-Rust CPU inference. Available on every supported platform.
    #[default]
    Cpu,
    /// Optional Intel OpenVINO execution provider (Windows and Linux x86-64). No backend ships
    /// it in v1; requesting it on a CPU-only build is a clear, non-fatal error.
    OpenVino,
}

impl Accelerator {
    /// Every accelerator this build knows, available or not, in preference order after CPU.
    pub const ALL: [Accelerator; 2] = [Accelerator::Cpu, Accelerator::OpenVino];

    pub fn as_str(self) -> &'static str {
        match self {
            Accelerator::Cpu => "cpu",
            Accelerator::OpenVino => "openvino",
        }
    }

    /// Parses `auto`, `cpu` or `openvino`.
    pub fn parse(input: &str) -> Result<Accelerator, String> {
        match input.trim().to_ascii_lowercase().as_str() {
            "" | "auto" | "default" => Ok(Accelerator::Cpu),
            "cpu" => Ok(Accelerator::Cpu),
            "openvino" | "npu" | "gpu" => Ok(Accelerator::OpenVino),
            other => Err(format!("`{other}` is not an accelerator; use auto, cpu or openvino")),
        }
    }

    /// Whether this build can run on it now.
    pub fn available(self) -> bool {
        match self {
            Accelerator::Cpu => true,
            // The optional OpenVINO backend announces itself through the environment until it
            // exists as its own crate. Nothing is loaded or probed at startup.
            Accelerator::OpenVino => openvino_announced(),
        }
    }

    /// The clear message for a requested accelerator this build cannot use. `None` when it is
    /// available.
    pub fn unavailable_message(self) -> Option<&'static str> {
        if self.available() { None } else { Some("this build runs translation on the CPU; the OpenVINO NPU/GPU backend is not available") }
    }
}

impl fmt::Display for Accelerator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Accelerators available right now, CPU first.
pub fn available() -> Vec<Accelerator> {
    Accelerator::ALL.into_iter().filter(|a| a.available()).collect()
}

fn openvino_announced() -> bool {
    std::env::var_os("GRIDCRAFT_OPENVINO").is_some_and(|v| !v.is_empty() && v != "0")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_is_always_available() {
        assert!(Accelerator::Cpu.available());
        assert_eq!(Accelerator::Cpu.unavailable_message(), None);
        assert!(available().contains(&Accelerator::Cpu));
    }

    #[test]
    fn parses_names() {
        assert_eq!(Accelerator::parse("auto"), Ok(Accelerator::Cpu));
        assert_eq!(Accelerator::parse("CPU"), Ok(Accelerator::Cpu));
        assert_eq!(Accelerator::parse("openvino"), Ok(Accelerator::OpenVino));
        assert!(Accelerator::parse("cuda").is_err());
    }

    #[test]
    fn requesting_openvino_on_a_cpu_build_is_actionable() {
        // The test environment does not announce OpenVINO.
        if let Some(message) = Accelerator::OpenVino.unavailable_message() {
            assert!(message.contains("runs translation on the CPU"));
        }
    }
}
