macro_rules! define_ids {
    ($($id:ident),* $(,)?) => {
        $(
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
            #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
            pub struct $id(pub u64);

            impl ::std::fmt::Display for $id {
                fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                    write!(f, "{}({})", stringify!($id), self.0)
                }
            }

            impl From<u64> for $id {
                fn from(v: u64) -> Self {
                    Self(v)
                }
            }

            impl $id {
                pub fn raw(self) -> u64 {
                    self.0
                }
            }
        )*
    };
}

define_ids!(TrackId, AlbumId, ArtistId, PlaylistId);
