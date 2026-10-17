use std::path::PathBuf;
use std::process::Command;

use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum ToolsPackagingError {
    #[error("Usage: package-plan|package-run <windows|macos|linux> | package-verify <windows|macos|linux> <directory> | notices-check")]
    Usage,
    #[error("The {0} package can only be assembled on its own operating system")]
    WrongHost(&'static str),
    #[error("Packaging step '{description}' failed with {status}")]
    Step {
        description: &'static str,
        status: std::process::ExitStatus,
    },
    #[error("Packaging I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("The AppImage toolchain gate is unresolved: {0}")]
    AppImageToolchain(String),
    #[error("Package verification failed:\n{0}")]
    Verification(String),
    #[error("The notices manifest is invalid: {0}")]
    Notices(String),
    #[error("Cannot decode JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Manifest(#[from] chess::engine::package::manifest::EnginePackageManifestError),
    #[error(transparent)]
    Digest(#[from] chess::package::digest::PackageDigestError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolsPackagingEnumPlatform {
    Windows,
    Macos,
    Linux,
}

impl ToolsPackagingEnumPlatform {
    pub(crate) fn parse(name: &str) -> Option<Self> {
        match name {
            "windows" => Some(Self::Windows),
            "macos" => Some(Self::Macos),
            "linux" => Some(Self::Linux),
            _ => None,
        }
    }

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Macos => "macos",
            Self::Linux => "linux",
        }
    }

    pub(crate) const fn is_current(self) -> bool {
        match self {
            Self::Windows => cfg!(target_os = "windows"),
            Self::Macos => cfg!(target_os = "macos"),
            Self::Linux => cfg!(target_os = "linux"),
        }
    }

    pub(crate) const fn engine_triple(self) -> &'static str {
        match self {
            Self::Windows => "x86_64-pc-windows-msvc",
            Self::Macos => "aarch64-apple-darwin",
            Self::Linux => "x86_64-unknown-linux-gnu",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolsPackagingStep {
    pub(crate) description: &'static str,
    pub(crate) program: String,
    pub(crate) arguments: Vec<String>,
}

impl ToolsPackagingStep {
    pub(crate) fn new(description: &'static str, program: &str, arguments: &[&str]) -> Self {
        Self {
            description,
            program: program.to_owned(),
            arguments: arguments.iter().map(|argument| (*argument).to_owned()).collect(),
        }
    }

    pub(crate) fn command_line(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.arguments.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolsPackagingPlan {
    pub(crate) platform: ToolsPackagingEnumPlatform,
    pub(crate) steps: Vec<ToolsPackagingStep>,
    pub(crate) artifacts: Vec<PathBuf>,
    pub(crate) guidance: &'static str,
}

impl ToolsPackagingPlan {
    pub(crate) fn describe(&self) -> String {
        let mut text = format!("Packaging plan for {}\n", self.platform.name());
        for (index, step) in self.steps.iter().enumerate() {
            text.push_str(&format!(
                "{}. {}: {}\n",
                index + 1,
                step.description,
                step.command_line()
            ));
        }
        for artifact in &self.artifacts {
            text.push_str(&format!("Artifact: {}\n", artifact.display()));
        }
        text.push_str(self.guidance);
        text.push('\n');
        text
    }

    pub(crate) fn run(&self) -> Result<(), ToolsPackagingError> {
        if !self.platform.is_current() {
            return Err(ToolsPackagingError::WrongHost(self.platform.name()));
        }
        for step in &self.steps {
            let status = Command::new(&step.program)
                .args(&step.arguments)
                .status()?;
            if !status.success() {
                return Err(ToolsPackagingError::Step {
                    description: step.description,
                    status,
                });
            }
        }
        Ok(())
    }
}
