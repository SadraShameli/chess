use thiserror::Error;

use crate::desktop::preferences::DesktopPreferencesEnumAudioCategory;
use crate::presentation::audio::router::PresentationAudioPlayback;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DesktopNativeAudioError {
    #[error("No audio output device is available")]
    DeviceUnavailable,
    #[error("The audio service refused playback")]
    ServiceFailed,
}

pub trait DesktopNativeAudioPort {
    fn play(
        &mut self,
        category: DesktopPreferencesEnumAudioCategory,
        gain: f32,
    ) -> Result<(), DesktopNativeAudioError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopNativeAudioWarning {
    error: DesktopNativeAudioError,
}

impl DesktopNativeAudioWarning {
    pub fn error(&self) -> &DesktopNativeAudioError {
        &self.error
    }
}

pub struct DesktopNativeAudio<P: DesktopNativeAudioPort> {
    port: P,
    warning: Option<DesktopNativeAudioWarning>,
}

impl<P: DesktopNativeAudioPort> DesktopNativeAudio<P> {
    pub const fn new(port: P) -> Self {
        Self {
            port,
            warning: None,
        }
    }

    pub fn warning(&self) -> Option<&DesktopNativeAudioWarning> {
        self.warning.as_ref()
    }

    pub fn play(&mut self, playback: PresentationAudioPlayback) {
        match self.port.play(playback.category(), playback.gain()) {
            Ok(()) => self.warning = None,
            Err(error) => self.warning = Some(DesktopNativeAudioWarning { error }),
        }
    }

    pub fn port(&self) -> &P {
        &self.port
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DesktopNativeAudioUnavailable;

impl DesktopNativeAudioPort for DesktopNativeAudioUnavailable {
    fn play(
        &mut self,
        _category: DesktopPreferencesEnumAudioCategory,
        _gain: f32,
    ) -> Result<(), DesktopNativeAudioError> {
        Err(DesktopNativeAudioError::DeviceUnavailable)
    }
}
