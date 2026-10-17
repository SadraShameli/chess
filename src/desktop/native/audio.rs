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

    pub fn port_mut(&mut self) -> &mut P {
        &mut self.port
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DesktopNativeAudioRequest {
    category: DesktopPreferencesEnumAudioCategory,
    gain: f32,
}

impl DesktopNativeAudioRequest {
    pub const fn category(self) -> DesktopPreferencesEnumAudioCategory {
        self.category
    }

    pub const fn gain(self) -> f32 {
        self.gain
    }
}

pub struct DesktopNativeAudioBevy {
    handles: [bevy::asset::Handle<bevy::audio::AudioSource>; 4],
    pending: Vec<DesktopNativeAudioRequest>,
    failed: bool,
}

impl DesktopNativeAudioBevy {
    pub const MOVE_ASSET: &'static str = "sounds/move.wav";
    pub const CHECK_ASSET: &'static str = "sounds/check.wav";
    pub const RESULT_ASSET: &'static str = "sounds/result.wav";
    pub const LOW_TIME_ASSET: &'static str = "sounds/low-time.wav";

    pub fn load(server: &bevy::asset::AssetServer) -> Self {
        Self {
            handles: [
                server.load(Self::MOVE_ASSET),
                server.load(Self::CHECK_ASSET),
                server.load(Self::RESULT_ASSET),
                server.load(Self::LOW_TIME_ASSET),
            ],
            pending: Vec::new(),
            failed: false,
        }
    }

    const fn slot(category: DesktopPreferencesEnumAudioCategory) -> usize {
        match category {
            DesktopPreferencesEnumAudioCategory::Move => 0,
            DesktopPreferencesEnumAudioCategory::Check => 1,
            DesktopPreferencesEnumAudioCategory::Result => 2,
            DesktopPreferencesEnumAudioCategory::LowTime => 3,
        }
    }

    pub fn refresh(&mut self, server: &bevy::asset::AssetServer) {
        self.failed = self.handles.iter().any(|handle| {
            matches!(
                server.get_load_state(handle.id()),
                Some(bevy::asset::LoadState::Failed(_))
            )
        });
    }

    pub fn drain(&mut self, world: &mut bevy::ecs::world::World) {
        for request in std::mem::take(&mut self.pending) {
            let handle = self.handles[Self::slot(request.category)].clone();
            world.spawn((
                bevy::audio::AudioPlayer::<bevy::audio::AudioSource>(handle),
                bevy::audio::PlaybackSettings::DESPAWN
                    .with_volume(bevy::audio::Volume::Linear(request.gain)),
            ));
        }
    }
}

impl DesktopNativeAudioPort for DesktopNativeAudioBevy {
    fn play(
        &mut self,
        category: DesktopPreferencesEnumAudioCategory,
        gain: f32,
    ) -> Result<(), DesktopNativeAudioError> {
        if self.failed {
            self.pending.clear();
            return Err(DesktopNativeAudioError::ServiceFailed);
        }
        self.pending.push(DesktopNativeAudioRequest { category, gain });
        Ok(())
    }
}
