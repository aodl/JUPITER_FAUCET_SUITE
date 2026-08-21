use crate::api::RelaySurplusRecipient;
use candid::Principal;
use ic_stable_structures::{storable::Bound, Storable};
use sha2::{Digest, Sha256};
use std::borrow::Cow;

const RELAY_CONFIGURATION_DOMAIN: &[u8] = b"jupiter-relay-configuration-v1\0";
const NEURON_RECIPIENT_MARKER: u8 = 0xff;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RelaySetupKey([u8; 32]);

impl RelaySetupKey {
    pub(super) fn from_canonical_configuration(
        targets: &[Principal],
        surplus_recipients: &[RelaySurplusRecipient],
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
            match recipient {
                RelaySurplusRecipient::Principal(principal) => {
                    let bytes = principal.as_slice();
                    // IC Principals are at most 29 bytes. Enforcing the stronger boundary here
                    // keeps 0xff permanently reserved for neuron recipients without changing the
                    // existing principal-only encoding.
                    assert!(bytes.len() < NEURON_RECIPIENT_MARKER as usize);
                    hasher.update([bytes.len() as u8]);
                    hasher.update(bytes);
                }
                RelaySurplusRecipient::Neuron(neuron_id) => {
                    hasher.update([NEURON_RECIPIENT_MARKER]);
                    hasher.update(neuron_id.to_be_bytes());
                }
            }
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
                vec![RelaySurplusRecipient::Principal(principal(2))],
                "f538a571cd5eb2306c1eb6ae79feef437df45da3bb8e29d186b2c9698ad142d0",
            ),
            (
                vec![principal(1), principal(2)],
                vec![RelaySurplusRecipient::Principal(principal(3))],
                "a16de158ab5cd2cb803f5c0b2d991fc6e07ad2345bf9a9ed4ad0a9e04c1e56c7",
            ),
            (
                vec![principal(1)],
                vec![
                    RelaySurplusRecipient::Principal(principal(2)),
                    RelaySurplusRecipient::Principal(principal(3)),
                ],
                "f3411192bc59b495f34669fdadc25411fc57bbc4224f5948fb3eadd15f487a0b",
            ),
            (
                vec![principal(2)],
                vec![RelaySurplusRecipient::Principal(principal(1))],
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
    fn neuron_and_mixed_hashes_have_stable_golden_vectors() {
        let principal = |byte| Principal::from_slice(&[byte]);
        assert_eq!(
            RelaySetupKey::from_canonical_configuration(
                &[principal(1)],
                &[RelaySurplusRecipient::Neuron(42)],
            )
            .identifier(),
            "7244d14aaf4640e08d19440c3f6a0e2aa7bb08b512b0dcd5b4fdf22795ff202b"
        );
        assert_eq!(
            RelaySetupKey::from_canonical_configuration(
                &[principal(1)],
                &[
                    RelaySurplusRecipient::Principal(principal(2)),
                    RelaySurplusRecipient::Neuron(42),
                ],
            )
            .identifier(),
            "7dd4b2e0d4247805d6148f4dbf7842660d8d888c2d17d9ae41353532dc03f1be"
        );
    }

    #[test]
    fn neuron_encoding_is_big_endian_and_type_separated() {
        let principal = Principal::from_slice(&[1]);
        let neuron_one = RelaySetupKey::from_canonical_configuration(
            &[principal],
            &[RelaySurplusRecipient::Neuron(1)],
        );
        let neuron_max = RelaySetupKey::from_canonical_configuration(
            &[principal],
            &[RelaySurplusRecipient::Neuron(u64::MAX)],
        );
        let principal_one = RelaySetupKey::from_canonical_configuration(
            &[principal],
            &[RelaySurplusRecipient::Principal(Principal::from_slice(&[
                1,
            ]))],
        );
        assert_ne!(neuron_one, neuron_max);
        assert_ne!(neuron_one, principal_one);
        assert_eq!(NEURON_RECIPIENT_MARKER, 0xff);
        assert!(
            Principal::from_slice(&[0; 29]).as_slice().len() < NEURON_RECIPIENT_MARKER as usize
        );
        assert_eq!(42u64.to_be_bytes(), [0, 0, 0, 0, 0, 0, 0, 42]);
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
