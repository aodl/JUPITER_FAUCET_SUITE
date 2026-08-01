use candid::Principal;
use ic_stable_structures::{storable::Bound, Storable};
use sha2::{Digest, Sha256};
use std::borrow::Cow;

const TARGET_SET_DOMAIN: &[u8] = b"jupiter-relay-target-set-v1\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RelaySetupKey([u8; 32]);

impl RelaySetupKey {
    pub(crate) fn from_canonical_targets(targets: &[Principal]) -> Self {
        debug_assert!(!targets.is_empty());
        debug_assert!(targets.len() <= u8::MAX as usize);
        let mut hasher = Sha256::new();
        hasher.update(TARGET_SET_DOMAIN);
        hasher.update([targets.len() as u8]);
        for target in targets {
            let bytes = target.as_slice();
            debug_assert!(bytes.len() <= u8::MAX as usize);
            hasher.update([bytes.len() as u8]);
            hasher.update(bytes);
        }
        Self(hasher.finalize().into())
    }

    pub(crate) fn bytes(self) -> [u8; 32] {
        self.0
    }

    pub(crate) fn identifier(self) -> String {
        hex::encode(self.0)
    }
}

impl Storable for RelaySetupKey {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Borrowed(&self.0)
    }

    fn into_bytes(self) -> Vec<u8> {
        self.0.to_vec()
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        Self(
            bytes
                .as_ref()
                .try_into()
                .expect("relay setup key must be exactly 32 bytes"),
        )
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 32,
        is_fixed_size: true,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framed_hash_has_stable_golden_vectors() {
        let singleton = RelaySetupKey::from_canonical_targets(&[Principal::from_slice(&[1])]);
        let pair = RelaySetupKey::from_canonical_targets(&[
            Principal::from_slice(&[1]),
            Principal::from_slice(&[2]),
        ]);
        assert_eq!(
            hex::encode(singleton.bytes()),
            "8e77e01e392fc0237e31f04f831a6c85c1da2a5f5f094bfef93a741cd79054d4"
        );
        assert_eq!(
            hex::encode(pair.bytes()),
            "beca1eef71e77a764b0818969fbe88f42af3d15e33ba0dc559f63415f2c5f8ee"
        );
        assert_ne!(singleton, pair);
    }
}
