use candid::Principal;
use ic_stable_structures::{storable::Bound, Storable};
use sha2::{Digest, Sha256};
use std::borrow::Cow;

const RELAY_CONFIGURATION_DOMAIN: &[u8] = b"jupiter-relay-configuration-v1\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RelaySetupKey([u8; 32]);

impl RelaySetupKey {
    pub(super) fn from_canonical_configuration(
        targets: &[Principal],
        surplus_recipients: &[Principal],
    ) -> Self {
        debug_assert!(!targets.is_empty());
        debug_assert!(targets.len() <= u8::MAX as usize);
        debug_assert!(!surplus_recipients.is_empty());
        debug_assert!(surplus_recipients.len() <= u8::MAX as usize);
        let mut hasher = Sha256::new();
        hasher.update(RELAY_CONFIGURATION_DOMAIN);
        hasher.update([0x01]);
        hasher.update([targets.len() as u8]);
        for target in targets {
            let bytes = target.as_slice();
            debug_assert!(bytes.len() <= u8::MAX as usize);
            hasher.update([bytes.len() as u8]);
            hasher.update(bytes);
        }
        hasher.update([0x02]);
        hasher.update([surplus_recipients.len() as u8]);
        for recipient in surplus_recipients {
            let bytes = recipient.as_slice();
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
    fn full_configuration_hash_has_stable_golden_vectors() {
        let principal = |byte| Principal::from_slice(&[byte]);
        let vectors = [
            (
                vec![principal(1)],
                vec![principal(2)],
                "f538a571cd5eb2306c1eb6ae79feef437df45da3bb8e29d186b2c9698ad142d0",
            ),
            (
                vec![principal(1), principal(2)],
                vec![principal(3)],
                "a16de158ab5cd2cb803f5c0b2d991fc6e07ad2345bf9a9ed4ad0a9e04c1e56c7",
            ),
            (
                vec![principal(1)],
                vec![principal(2), principal(3)],
                "f3411192bc59b495f34669fdadc25411fc57bbc4224f5948fb3eadd15f487a0b",
            ),
            (
                vec![principal(2)],
                vec![principal(1)],
                "068852f00f965076725a365fc5c509e465c28073c1983db5c54c85dca51f0273",
            ),
        ];
        for (targets, recipients, expected) in vectors {
            assert_eq!(
                RelaySetupKey::from_canonical_configuration(&targets, &recipients).identifier(),
                expected
            );
        }
    }

    #[test]
    fn relay_setup_key_is_a_fixed_32_byte_stable_key() {
        assert_eq!(
            RelaySetupKey::BOUND,
            Bound::Bounded {
                max_size: 32,
                is_fixed_size: true
            }
        );
    }
}
