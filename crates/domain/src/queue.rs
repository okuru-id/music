use crate::errors::DomainError;
use crate::ids::TrackId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RepeatMode {
    #[default]
    Off,
    One,
    All,
}

/// Play queue aggregate root. Invariant: `current` always indexes a valid item
/// (or the queue is empty and `current == 0`).
///
/// Shuffle is a pure transformation: randomness is injected via `rng: &mut impl FnMut() -> u64`,
/// so queue logic stays deterministic and unit-testable. `original` keeps the pre-shuffle
/// order so unshuffle restores it exactly.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayQueue {
    original: Vec<TrackId>,
    items: Vec<TrackId>,
    current: usize,
    shuffled: bool,
    repeat: RepeatMode,
}

impl Default for PlayQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl PlayQueue {
    pub fn new() -> Self {
        Self {
            original: Vec::new(),
            items: Vec::new(),
            current: 0,
            shuffled: false,
            repeat: RepeatMode::Off,
        }
    }

    pub fn replace_all(&mut self, items: Vec<TrackId>) {
        self.items = items.clone();
        self.original = items;
        self.current = 0;
        self.shuffled = false;
    }

    pub fn items(&self) -> &[TrackId] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn current_index(&self) -> usize {
        self.current
    }

    pub fn current(&self) -> Option<TrackId> {
        self.items.get(self.current).copied()
    }

    pub fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    pub fn set_repeat(&mut self, mode: RepeatMode) {
        self.repeat = mode;
    }

    pub fn is_shuffled(&self) -> bool {
        self.shuffled
    }

    pub fn jump_to(&mut self, index: usize) -> Result<TrackId, DomainError> {
        if index >= self.items.len() {
            return Err(DomainError::IndexOutOfRange(index));
        }
        self.current = index;
        Ok(self.items[index])
    }

    /// Advance according to repeat mode. `RepeatMode::One` replays the current track.
    pub fn next_track(&mut self) -> Option<TrackId> {
        if self.items.is_empty() {
            return None;
        }
        if self.repeat == RepeatMode::One {
            return Some(self.items[self.current]);
        }
        if self.current + 1 < self.items.len() {
            self.current += 1;
            Some(self.items[self.current])
        } else if self.repeat == RepeatMode::All {
            self.current = 0;
            Some(self.items[0])
        } else {
            None
        }
    }

    /// Go back. At index 0 with `RepeatMode::Off` returns None (UI may seek-to-start instead).
    pub fn previous_track(&mut self) -> Option<TrackId> {
        if self.items.is_empty() {
            return None;
        }
        if self.current > 0 {
            self.current -= 1;
            Some(self.items[self.current])
        } else if self.repeat == RepeatMode::All {
            self.current = self.items.len() - 1;
            Some(self.items[self.current])
        } else {
            None
        }
    }

    /// Append to the end of the queue.
    pub fn add(&mut self, track: TrackId) {
        self.items.push(track);
        if !self.shuffled {
            self.original.push(track);
        }
    }

    /// Insert right after the current track ("play next").
    pub fn queue_next(&mut self, track: TrackId) {
        let at = (self.current + 1).min(self.items.len());
        self.items.insert(at, track);
        if !self.shuffled {
            self.original.insert(at, track);
        }
    }

    /// Remove by index, keeping `current` valid and pointing at the same
    /// playing position (next item if the current one is removed).
    pub fn remove(&mut self, index: usize) -> Result<TrackId, DomainError> {
        if index >= self.items.len() {
            return Err(DomainError::IndexOutOfRange(index));
        }
        let removed = self.items.remove(index);
        // ponytail: duplicate TrackIds make original-order restore ambiguous;
        // we drop the first matching occurrence, fine for typical libraries.
        if let Some(p) = self.original.iter().position(|t| *t == removed) {
            self.original.remove(p);
        }
        if self.items.is_empty() {
            self.current = 0;
        } else if index < self.current {
            self.current -= 1;
        } else {
            self.current = self.current.min(self.items.len() - 1);
        }
        Ok(removed)
    }

    /// Shuffle, keeping the current track first. Randomness injected by caller.
    pub fn shuffle(&mut self, rng: &mut impl FnMut() -> u64) {
        if self.items.len() < 2 {
            self.shuffled = true;
            return;
        }
        let current_id = self.items[self.current];
        let mut rest: Vec<TrackId> = self
            .items
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != self.current)
            .map(|(_, t)| *t)
            .collect();
        // Fisher–Yates.
        for i in (1..rest.len()).rev() {
            let j = (rng() % (i as u64 + 1)) as usize;
            rest.swap(i, j);
        }
        let mut new_items = Vec::with_capacity(rest.len() + 1);
        new_items.push(current_id);
        new_items.extend(rest);
        self.items = new_items;
        self.current = 0;
        self.shuffled = true;
    }

    /// Restore the original (pre-shuffle) order, keeping the current track.
    pub fn unshuffle(&mut self) {
        if !self.shuffled {
            return;
        }
        let current_id = self.items[self.current];
        self.items = self.original.clone();
        self.current = self
            .items
            .iter()
            .position(|t| *t == current_id)
            .unwrap_or(0);
        self.shuffled = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(n: u64) -> Vec<TrackId> {
        (1..=n).map(TrackId).collect()
    }

    fn rng() -> impl FnMut() -> u64 {
        let mut s = 42u64;
        move || {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            s
        }
    }

    #[test]
    fn next_linear_off_stops_at_end() {
        let mut q = PlayQueue::new();
        assert_eq!(q.next_track(), None);
        q.replace_all(ids(3));
        assert_eq!(q.current(), Some(TrackId(1)));
        assert_eq!(q.next_track(), Some(TrackId(2)));
        assert_eq!(q.next_track(), Some(TrackId(3)));
        assert_eq!(q.next_track(), None);
    }

    #[test]
    fn next_wrap_all_and_repeat_one() {
        let mut q = PlayQueue::new();
        q.replace_all(ids(2));
        q.set_repeat(RepeatMode::All);
        assert_eq!(q.next_track(), Some(TrackId(2)));
        assert_eq!(q.next_track(), Some(TrackId(1)));

        q.set_repeat(RepeatMode::One);
        q.jump_to(1).unwrap();
        assert_eq!(q.next_track(), Some(TrackId(2)));
        assert_eq!(q.next_track(), Some(TrackId(2)));
    }

    #[test]
    fn previous() {
        let mut q = PlayQueue::new();
        q.replace_all(ids(2));
        assert_eq!(q.previous_track(), None);
        q.set_repeat(RepeatMode::All);
        assert_eq!(q.previous_track(), Some(TrackId(2)));
        assert_eq!(q.previous_track(), Some(TrackId(1)));
    }

    #[test]
    fn shuffle_preserves_items_and_current() {
        let mut q = PlayQueue::new();
        q.replace_all(ids(20));
        q.jump_to(7).unwrap();
        let before = q.current();
        let mut r = rng();
        q.shuffle(&mut r);
        assert_eq!(q.current(), before);
        assert_eq!(q.current_index(), 0);
        let mut sorted: Vec<_> = q.items().to_vec();
        sorted.sort();
        assert_eq!(sorted, ids(20));
        q.unshuffle();
        assert_eq!(q.items(), &ids(20));
        assert_eq!(q.current(), before);
        assert!(!q.is_shuffled());
    }

    #[test]
    fn remove_keeps_current_valid() {
        let mut q = PlayQueue::new();
        q.replace_all(ids(3));
        q.jump_to(2).unwrap();
        q.remove(0).unwrap();
        assert_eq!(q.current(), Some(TrackId(3)));
        q.remove(1).unwrap();
        // removing the playing track falls back to the previous item
        assert_eq!(q.current(), Some(TrackId(2)));
        assert_eq!(q.remove(5), Err(DomainError::IndexOutOfRange(5)));
        q.remove(0).unwrap();
        assert!(q.is_empty());
        assert_eq!(q.current(), None);
    }

    #[test]
    fn queue_next_inserts_after_current() {
        let mut q = PlayQueue::new();
        q.replace_all(ids(3));
        q.queue_next(TrackId(9));
        // inserted right after current (index 0)
        assert_eq!(q.items(), &[TrackId(1), TrackId(9), TrackId(2), TrackId(3)]);
        q.jump_to(0).unwrap();
        q.next_track();
        q.queue_next(TrackId(8));
        assert_eq!(q.items()[2], TrackId(8));
    }
}
