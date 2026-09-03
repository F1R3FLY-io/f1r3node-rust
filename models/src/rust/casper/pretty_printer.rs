// See models/src/main/scala/coop/rchain/casper/PrettyPrinter.scala

use crypto::rust::signatures::signed::Signed;
use shared::rust::ByteString;

use super::protocol::casper_message::{
    BlockHashMessage, BlockMessage, Bond, CasperMessage, DeployData, F1r3flyState, ProcessedDeploy,
};

pub struct PrettyPrinter;

impl PrettyPrinter {
    pub fn build_string_no_limit(b: &[u8]) -> String { hex::encode(b) }

    pub fn build_string(t: CasperMessage, short: bool) -> String {
        match t {
            CasperMessage::BlockMessage(block_message) => {
                Self::build_string_block_message(&block_message, short)
            }
            _ => "Unknown consensus protocol message".to_string(),
        }
    }

    pub fn build_string_block_message(b: &BlockMessage, short: bool) -> String {
        match b.header.parents_hash_list.first() {
            None => format!(
                "Block #{} ({}) with empty parents (supposedly genesis)",
                b.body.state.block_number,
                Self::build_string_bytes(&b.block_hash)
            ),
            Some(main_parent) => {
                if short {
                    format!(
                        "#{} ({})",
                        b.body.state.block_number,
                        Self::build_string_bytes(&b.block_hash)
                    )
                } else {
                    format!(
                      "Block #{} ({}) -- Sender ID {} -- M Parent Hash {} -- Contents {} -- Shard ID {}",
                      b.body.state.block_number,
                      Self::build_string_bytes(&b.block_hash),
                      Self::build_string_bytes(&b.sender),
                      Self::build_string_bytes(main_parent),
                      Self::build_string_f1r3fly_state(&b.body.state),
                      Self::limit(&b.shard_id, 10)
                  )
                }
            }
        }
    }

    pub fn build_string_block_hash_message(bh: &BlockHashMessage) -> String {
        format!("Block hash: {}", Self::build_string_bytes(&bh.block_hash))
    }

    fn limit(s: &str, max_length: usize) -> String {
        if s.len() > max_length {
            format!("{}...", &s[0..max_length])
        } else {
            s.to_string()
        }
    }

    pub fn build_string_processed_deploy(d: &ProcessedDeploy) -> String {
        format!(
            "User: {}, Cost: {:?} {}",
            Self::build_string_no_limit(&d.deploy.pk.bytes),
            d.cost,
            Self::build_string_signed_deploy_data(&d.deploy)
        )
    }

    pub fn build_string_bytes(bytes: &[u8]) -> String { Self::limit(&hex::encode(bytes), 10) }

    pub fn build_string_sig(bytes: &[u8]) -> String {
        let str1 = hex::encode(&bytes[0..10.min(bytes.len())]);
        let str2 = if bytes.len() > 10 {
            hex::encode(&bytes[bytes.len() - 10.min(bytes.len())..])
        } else {
            "".to_string()
        };
        format!("{}...{}", str1, str2)
    }

    pub fn build_string_signed_deploy_data(sd: &Signed<DeployData>) -> String {
        format!(
            "{} Sig: {} SigAlgorithm: {} ValidAfterBlockNumber: {}",
            Self::build_string_deploy_data(&sd.data),
            Self::build_string_sig(&sd.sig),
            sd.sig_algorithm.name(),
            sd.data.valid_after_block_number
        )
    }

    pub fn build_string_deploy_data(d: &DeployData) -> String {
        format!("DeployData #{} -- {}", d.time_stamp, d.term)
    }

    pub fn build_string_f1r3fly_state(r: &F1r3flyState) -> String {
        Self::build_string_bytes(&r.post_state_hash)
    }

    pub fn build_string_bond(b: &Bond) -> String {
        format!("{}: {}", Self::build_string_no_limit(&b.validator), b.stake)
    }

    pub fn build_string_hashes(hashes: &[ByteString]) -> String {
        let contents: Vec<String> = hashes
            .iter()
            .map(|hash| Self::build_string_bytes(hash))
            .collect();
        format!("[{}]", contents.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use prost::bytes::Bytes;

    use super::*;
    use crate::rhoapi::PCost;
    use crate::rust::block_implicits::get_random_block;
    use crate::rust::casper::protocol::casper_message::{DeployAdmissionStatus, HasBlock};

    fn block_with_parents(parents: Vec<Bytes>) -> BlockMessage {
        get_random_block(
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(parents),
            None,
            None,
            None,
            None,
            None,
            None,
        )
    }

    fn signed_deploy() -> Signed<DeployData> {
        let secp256k1 = Secp256k1;
        let (sec, _) = secp256k1.new_key_pair();
        Signed::create(
            DeployData {
                term: "new x in { x!(1) }".to_string(),
                language: "rholang".to_string(),
                time_stamp: 42,
                valid_after_block_number: 5,
                shard_id: "root".to_string(),
                expiration_timestamp: None,
                authority_presentations: Vec::new(),
            },
            Box::new(secp256k1),
            sec,
        )
        .unwrap()
    }

    #[test]
    fn build_string_no_limit_hex_encodes_all_bytes() {
        assert_eq!(
            PrettyPrinter::build_string_no_limit(&[0xde, 0xad, 0xbe, 0xef, 0x01, 0x02]),
            "deadbeef0102"
        );
    }

    #[test]
    fn build_string_bytes_truncates_hex_to_ten_chars() {
        assert_eq!(
            PrettyPrinter::build_string_bytes(&[0xab; 6]),
            "ababababab..."
        );
        assert_eq!(PrettyPrinter::build_string_bytes(&[0xab]), "ab");
        assert_eq!(PrettyPrinter::build_string_bytes(&[]), "");
    }

    #[test]
    fn build_string_sig_shows_head_and_tail_of_long_signature() {
        let bytes: Vec<u8> = (0u8..12).collect();
        let expected = format!(
            "{}...{}",
            hex::encode(&bytes[0..10]),
            hex::encode(&bytes[2..12])
        );
        assert_eq!(PrettyPrinter::build_string_sig(&bytes), expected);
    }

    #[test]
    fn build_string_sig_omits_tail_for_short_signature() {
        assert_eq!(PrettyPrinter::build_string_sig(&[1, 2, 3]), "010203...");
    }

    #[test]
    fn parentless_block_prints_as_genesis() {
        let block = block_with_parents(vec![]);
        let rendered = PrettyPrinter::build_string_block_message(&block, false);
        assert_eq!(
            rendered,
            format!(
                "Block #{} ({}) with empty parents (supposedly genesis)",
                block.body.state.block_number,
                PrettyPrinter::build_string_bytes(&block.block_hash)
            )
        );
    }

    #[test]
    fn short_form_prints_number_and_hash_only() {
        let block = block_with_parents(vec![Bytes::from_static(&[0x11; 32])]);
        let rendered = PrettyPrinter::build_string_block_message(&block, true);
        assert_eq!(
            rendered,
            format!(
                "#{} ({})",
                block.body.state.block_number,
                PrettyPrinter::build_string_bytes(&block.block_hash)
            )
        );
    }

    #[test]
    fn long_form_includes_sender_parent_state_and_shard() {
        let block = block_with_parents(vec![Bytes::from_static(&[0x22; 32])]);
        let rendered = PrettyPrinter::build_string_block_message(&block, false);
        assert!(rendered.contains(&format!(
            "Sender ID {}",
            PrettyPrinter::build_string_bytes(&block.sender)
        )));
        assert!(rendered.contains(&format!(
            "M Parent Hash {}",
            PrettyPrinter::build_string_bytes(&block.header.parents_hash_list[0])
        )));
        assert!(rendered.contains(&format!(
            "Contents {}",
            PrettyPrinter::build_string_bytes(&block.body.state.post_state_hash)
        )));
        assert!(rendered.contains("Shard ID root"));
    }

    #[test]
    fn build_string_only_renders_block_messages() {
        let block = block_with_parents(vec![]);
        let rendered =
            PrettyPrinter::build_string(CasperMessage::BlockMessage(block.clone()), false);
        assert_eq!(
            rendered,
            PrettyPrinter::build_string_block_message(&block, false)
        );

        let other = CasperMessage::HasBlock(HasBlock {
            hash: Bytes::from_static(b"h"),
        });
        assert_eq!(
            PrettyPrinter::build_string(other, false),
            "Unknown consensus protocol message"
        );
    }

    #[test]
    fn build_string_block_hash_message_prints_truncated_hash() {
        let message = BlockHashMessage {
            block_hash: Bytes::from_static(&[0xcd; 8]),
            block_creator: Bytes::new(),
        };
        assert_eq!(
            PrettyPrinter::build_string_block_hash_message(&message),
            "Block hash: cdcdcdcdcd..."
        );
    }

    #[test]
    fn build_string_bond_prints_full_validator_and_stake() {
        let bond = Bond {
            validator: Bytes::from_static(&[0xaa, 0xbb]),
            stake: 150,
        };
        assert_eq!(PrettyPrinter::build_string_bond(&bond), "aabb: 150");
    }

    #[test]
    fn build_string_hashes_joins_truncated_hashes() {
        let hashes: Vec<ByteString> = vec![vec![0x01; 6], vec![0x02]];
        assert_eq!(
            PrettyPrinter::build_string_hashes(&hashes),
            "[0101010101... 02]"
        );
    }

    #[test]
    fn build_string_deploy_data_prints_timestamp_and_term() {
        let deploy = DeployData {
            term: "Nil".to_string(),
            language: "rholang".to_string(),
            time_stamp: 99,
            valid_after_block_number: 0,
            shard_id: "root".to_string(),
            expiration_timestamp: None,
            authority_presentations: Vec::new(),
        };
        assert_eq!(
            PrettyPrinter::build_string_deploy_data(&deploy),
            "DeployData #99 -- Nil"
        );
    }

    #[test]
    fn signed_deploy_rendering_includes_signature_and_algorithm() {
        let signed = signed_deploy();
        let rendered = PrettyPrinter::build_string_signed_deploy_data(&signed);
        assert!(rendered.starts_with("DeployData #42 -- new x in { x!(1) }"));
        assert!(rendered.contains(&format!(
            "Sig: {}",
            PrettyPrinter::build_string_sig(&signed.sig)
        )));
        assert!(rendered.contains("SigAlgorithm: secp256k1"));
        assert!(rendered.contains("ValidAfterBlockNumber: 5"));
    }

    #[test]
    fn processed_deploy_rendering_includes_deployer_and_cost() {
        let signed = signed_deploy();
        let processed = ProcessedDeploy {
            deploy: signed.clone(),
            envelope_commitment: Bytes::new(),
            cost: PCost { cost: 17 },
            deploy_log: Vec::new(),
            is_failed: false,
            system_deploy_error: None,
            cosigners: Vec::new(),
            cosigner_threshold: 0,
            pre_state_hash: Bytes::new(),
            post_state_hash: Bytes::new(),
            authority_funding_certificate: None,
            authority_cost_witness: None,
            admission_status: DeployAdmissionStatus::Executed,
        };
        let rendered = PrettyPrinter::build_string_processed_deploy(&processed);
        assert!(rendered.starts_with(&format!(
            "User: {}",
            PrettyPrinter::build_string_no_limit(&signed.pk.bytes)
        )));
        assert!(rendered.contains("17"));
    }
}
