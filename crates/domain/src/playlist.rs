use crate::errors::DomainError;
use crate::ids::{PlaylistId, TrackId};

/// Aggregate root. Invariants: name non-empty, order preserved.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Playlist {
    pub id: PlaylistId,
    name: String,
    tracks: Vec<TrackId>,
}

impl Playlist {
    pub fn new(id: PlaylistId, name: impl Into<String>) -> Result<Self, DomainError> {
        Self::build(id, name, Vec::new())
    }

    pub fn build(
        id: PlaylistId,
        name: impl Into<String>,
        tracks: Vec<TrackId>,
    ) -> Result<Self, DomainError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(DomainError::EmptyPlaylistName);
        }
        Ok(Self { id, name, tracks })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn rename(&mut self, name: impl Into<String>) -> Result<(), DomainError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(DomainError::EmptyPlaylistName);
        }
        self.name = name;
        Ok(())
    }

    pub fn tracks(&self) -> &[TrackId] {
        &self.tracks
    }

    pub fn add(&mut self, track: TrackId) {
        self.tracks.push(track);
    }

    pub fn remove(&mut self, index: usize) -> Result<TrackId, DomainError> {
        if index >= self.tracks.len() {
            return Err(DomainError::IndexOutOfRange(index));
        }
        Ok(self.tracks.remove(index))
    }

    pub fn move_track(&mut self, from: usize, to: usize) -> Result<(), DomainError> {
        if from >= self.tracks.len() || to >= self.tracks.len() {
            return Err(DomainError::IndexOutOfRange(from.max(to)));
        }
        let t = self.tracks.remove(from);
        self.tracks.insert(to, t);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_name() {
        assert_eq!(
            Playlist::new(PlaylistId(1), "  "),
            Err(DomainError::EmptyPlaylistName)
        );
    }

    #[test]
    fn move_and_remove() {
        let mut p = Playlist::build(
            PlaylistId(1),
            "mix",
            vec![TrackId(1), TrackId(2), TrackId(3)],
        )
        .unwrap();
        p.move_track(0, 2).unwrap();
        assert_eq!(p.tracks(), &[TrackId(2), TrackId(3), TrackId(1)]);
        p.remove(1).unwrap();
        assert_eq!(p.tracks(), &[TrackId(2), TrackId(1)]);
        assert!(p.remove(5).is_err());
        assert!(p.move_track(0, 9).is_err());
    }
}
