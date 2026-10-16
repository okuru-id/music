use crate::ids::{AlbumId, ArtistId};

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Album {
    pub id: AlbumId,
    pub title: String,
    pub album_artist: Option<String>,
    pub year: Option<u16>,
    /// Path or cache key of the cover image.
    pub cover_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Artist {
    pub id: ArtistId,
    pub name: String,
}
