use crate::api::RelaySurplusRecipient;
use candid::Principal;
use ic_stable_structures::{storable::Bound, Storable};
use sha2::{Digest, Sha256};
use std::borrow::Cow;

pub(crate) const RELAY_CONFIGURATION_DOMAIN: &[u8] = b"jupiter-relay-configuration-v1\0";
const TARGET_SECTION_TAG: u8 = 0x01;
const RECIPIENT_SECTION_TAG: u8 = 0x02;
const PRINCIPAL_RECIPIENT_TAG: u8 = 0x01;
const NEURON_RECIPIENT_TAG: u8 = 0x02;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RelaySetupKey([u8; 32]);

impl RelaySetupKey {
    pub(super) fn from_canonical_configuration(
        targets: &[Principal],
        surplus_recipients: &[RelaySurplusRecipient],
    ) -> Self {
        Self(Sha256::digest(configuration_preimage(targets, surplus_recipients)).into())
    }

    pub(crate) fn bytes(self) -> [u8; 32] {
        self.0
    }

    pub(crate) fn identifier(self) -> String {
        hex::encode(self.0)
    }
}

fn configuration_preimage(
    targets: &[Principal],
    surplus_recipients: &[RelaySurplusRecipient],
) -> Vec<u8> {
    let target_count =
        u8::try_from(targets.len()).expect("validated Relay target count must fit u8");
    let recipient_count = u8::try_from(surplus_recipients.len())
        .expect("validated Relay surplus-recipient count must fit u8");

    let mut preimage = Vec::new();
    preimage.extend_from_slice(RELAY_CONFIGURATION_DOMAIN);
    preimage.push(TARGET_SECTION_TAG);
    preimage.push(target_count);
    for target in targets {
        let bytes = target.as_slice();
        preimage.push(
            u8::try_from(bytes.len()).expect("validated Relay target principal length must fit u8"),
        );
        preimage.extend_from_slice(bytes);
    }

    preimage.push(RECIPIENT_SECTION_TAG);
    preimage.push(recipient_count);
    for recipient in surplus_recipients {
        match recipient {
            RelaySurplusRecipient::Principal { principal, memo } => {
                let bytes = principal.as_slice();
                preimage.push(PRINCIPAL_RECIPIENT_TAG);
                preimage.push(
                    u8::try_from(bytes.len())
                        .expect("validated Relay recipient principal length must fit u8"),
                );
                preimage.extend_from_slice(bytes);
                preimage.push(
                    u8::try_from(memo.len())
                        .expect("validated Relay recipient memo length must fit u8"),
                );
                preimage.extend_from_slice(memo);
            }
            RelaySurplusRecipient::Neuron { neuron_id, memo } => {
                preimage.push(NEURON_RECIPIENT_TAG);
                preimage.extend_from_slice(&neuron_id.to_be_bytes());
                preimage.push(
                    u8::try_from(memo.len())
                        .expect("validated Relay recipient memo length must fit u8"),
                );
                preimage.extend_from_slice(memo);
            }
        }
    }
    preimage
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

    fn principal(byte: u8) -> Principal {
        Principal::from_slice(&[byte])
    }

    fn principal_recipient(byte: u8, memo: Vec<u8>) -> RelaySurplusRecipient {
        RelaySurplusRecipient::Principal {
            principal: principal(byte),
            memo,
        }
    }

    fn neuron_recipient(neuron_id: u64, memo: Vec<u8>) -> RelaySurplusRecipient {
        RelaySurplusRecipient::Neuron { neuron_id, memo }
    }

    #[test]
    fn canonical_preimages_and_hashes_match_golden_vectors() {
        let vectors = [
            (
                vec![principal(1)],
                vec![],
                "6a7570697465722d72656c61792d636f6e66696775726174696f6e2d763100010101010200",
                "49700876356a6229a8dfa8a8379a844a5545eda72e8083b96c740fc431495c04",
            ),
            (
                vec![principal(1)],
                vec![principal_recipient(2, vec![])],
                "6a7570697465722d72656c61792d636f6e66696775726174696f6e2d76310001010101020101010200",
                "3a0ed735d092e75bcd929d71235e2b3757af7e7c50d342427592cbf1a1bb0b1f",
            ),
            (
                vec![principal(1)],
                vec![principal_recipient(2, vec![0x00, 0xff])],
                "6a7570697465722d72656c61792d636f6e66696775726174696f6e2d7631000101010102010101020200ff",
                "953bb6f741d0fd96d989a53d442f9ef919ba92c5848c7b328cd2585af634087f",
            ),
            (
                vec![principal(1)],
                vec![neuron_recipient(42, vec![0x55; 32])],
                "6a7570697465722d72656c61792d636f6e66696775726174696f6e2d76310001010101020102000000000000002a205555555555555555555555555555555555555555555555555555555555555555",
                "97d68d2705438ae59b7c94b40e4d36d775b6487d28b0ffa45caaa57f98c021a1",
            ),
            (
                vec![principal(1), principal(2)],
                vec![
                    principal_recipient(3, vec![0x10]),
                    neuron_recipient(7, vec![0x20, 0x21]),
                ],
                "6a7570697465722d72656c61792d636f6e66696775726174696f6e2d76310001020101010202020101030110020000000000000007022021",
                "184f8586f6385a1649b1980017e2537cd1948488f42a260e92abc57f803c5dca",
            ),
        ];
        for (targets, recipients, expected_preimage, expected_hash) in vectors {
            let preimage = configuration_preimage(&targets, &recipients);
            assert_eq!(hex::encode(&preimage), expected_preimage);
            assert_eq!(hex::encode(Sha256::digest(&preimage)), expected_hash);
            assert_eq!(
                RelaySetupKey::from_canonical_configuration(&targets, &recipients).identifier(),
                expected_hash
            );
        }
    }

    #[test]
    fn identity_is_exact_byte_framed() {
        let targets = [principal(1), principal(2)];
        let empty = RelaySetupKey::from_canonical_configuration(
            &targets,
            &[principal_recipient(3, vec![])],
        );
        let zero = RelaySetupKey::from_canonical_configuration(
            &targets,
            &[principal_recipient(3, vec![0])],
        );
        let one = RelaySetupKey::from_canonical_configuration(
            &targets,
            &[principal_recipient(3, vec![1])],
        );
        assert_ne!(empty, zero);
        assert_ne!(zero, one);
        assert_ne!(
            one,
            RelaySetupKey::from_canonical_configuration(&targets, &[neuron_recipient(3, vec![1])])
        );
        assert_ne!(
            one,
            RelaySetupKey::from_canonical_configuration(
                &targets,
                &[principal_recipient(4, vec![1])]
            )
        );
        assert_ne!(
            RelaySetupKey::from_canonical_configuration(&targets, &[neuron_recipient(3, vec![1])]),
            RelaySetupKey::from_canonical_configuration(&targets, &[neuron_recipient(4, vec![1])])
        );
        assert_ne!(
            one,
            RelaySetupKey::from_canonical_configuration(
                &[principal(1), principal(9)],
                &[principal_recipient(3, vec![1])]
            )
        );

        let left = configuration_preimage(&[principal(1)], &[principal_recipient(2, vec![3, 4])]);
        let right = configuration_preimage(
            &[principal(1)],
            &[RelaySurplusRecipient::Principal {
                principal: Principal::from_slice(&[2, 3]),
                memo: vec![4],
            }],
        );
        assert_ne!(left, right);
    }

    #[test]
    fn identical_exact_bytes_have_identical_identity() {
        let text_bytes = "memo 123".as_bytes().to_vec();
        let hexadecimal_bytes = hex::decode("6d656d6f20313233").unwrap();
        assert_eq!(text_bytes, hexadecimal_bytes);
        assert_eq!(
            RelaySetupKey::from_canonical_configuration(
                &[principal(1)],
                &[principal_recipient(2, text_bytes)]
            ),
            RelaySetupKey::from_canonical_configuration(
                &[principal(1)],
                &[principal_recipient(2, hexadecimal_bytes)]
            )
        );
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
