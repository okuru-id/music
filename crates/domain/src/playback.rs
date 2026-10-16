use std::time::Duration;

use crate::errors::DomainError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackStatus {
    Stopped,
    Playing,
    Paused,
}

/// Volume 0.0–1.0. Validated on construction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Volume(f32);

impl Volume {
    pub fn try_new(v: f32) -> Result<Self, DomainError> {
        if !(0.0..=1.0).contains(&v) || v.is_nan() {
            return Err(DomainError::VolumeOutOfRange(v));
        }
        Ok(Self(v))
    }

    /// Clamp instead of reject — for raw slider/hardware input.
    pub fn clamped(v: f32) -> Self {
        Self(v.clamp(0.0, 1.0))
    }

    pub fn inner(self) -> f32 {
        self.0
    }
}

impl Default for Volume {
    fn default() -> Self {
        Self(1.0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlaybackState {
    pub status: PlaybackStatus,
    pub position: Duration,
    pub volume: Volume,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            status: PlaybackStatus::Stopped,
            position: Duration::ZERO,
            volume: Volume::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_bounds() {
        assert!(Volume::try_new(0.5).is_ok());
        assert_eq!(
            Volume::try_new(1.5),
            Err(DomainError::VolumeOutOfRange(1.5))
        );
        assert!(matches!(
            Volume::try_new(f32::NAN),
            Err(DomainError::VolumeOutOfRange(_))
        ));
        assert_eq!(Volume::clamped(2.0).inner(), 1.0);
        assert_eq!(Volume::clamped(-1.0).inner(), 0.0);
    }
}
