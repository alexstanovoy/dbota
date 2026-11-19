//! Ownership shared by kept replays and the match archive.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Scope {
    /// Existing rows and files that never recorded an owner.
    Unattributed,
    #[default]
    Guest,
    Account([u8; 32]),
}

impl Scope {
    pub fn account(service: &str, account: u64) -> Self {
        let Some(service) = bota_radovid::matchmaker_identity(service) else {
            return Self::Unattributed;
        };
        let mut key = blake3::Hasher::new();
        key.update(&account.to_le_bytes());
        key.update(service.as_bytes());
        Self::Account(*key.finalize().as_bytes())
    }

    pub(crate) fn unattributed() -> Self {
        Self::Unattributed
    }

    pub(crate) fn includes(self, owner: Self) -> bool {
        self != Self::Unattributed && self == owner
    }

    pub(crate) fn index_field(self) -> String {
        match self {
            Self::Unattributed => "?".to_owned(),
            Self::Guest => "-".to_owned(),
            Self::Account(key) => blake3::Hash::from_bytes(key).to_hex().to_string(),
        }
    }

    pub(super) fn from_index(field: &str) -> Self {
        match field {
            "-" => Self::Guest,
            _ => blake3::Hash::from_hex(field)
                .map_or(Self::Unattributed, |key| Self::Account(*key.as_bytes())),
        }
    }
}
