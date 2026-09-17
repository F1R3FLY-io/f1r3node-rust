use std::num::NonZeroUsize;

use serde::{Deserialize, Serialize};

use super::{
    MonetaryCohort, MonetaryCohortError, MonetaryCohortPlan, MonetaryCursor,
    MonetaryCursorTransition,
};

pub const MONETARY_FEE_POLICY_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonetaryFeeFields {
    pub policy_version: u32,
    pub policy_context: [u8; 32],
    pub scope: [u8; 32],
    pub payer_custodies: Vec<[u8; 32]>,
    pub obligation: u64,
    pub expected_revision: i64,
    pub expected_position: i64,
    pub next_revision: i64,
    pub next_position: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "MonetaryFeeFields", into = "MonetaryFeeFields")]
pub struct MonetaryFeeEvidence {
    fields: MonetaryFeeFields,
}

impl TryFrom<MonetaryFeeFields> for MonetaryFeeEvidence {
    type Error = MonetaryCohortError;

    fn try_from(fields: MonetaryFeeFields) -> Result<Self, Self::Error> {
        if fields.policy_version != MONETARY_FEE_POLICY_VERSION
            || fields.obligation == 0
            || fields.payer_custodies.is_empty()
            || fields
                .payer_custodies
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || fields.scope
                != super::cohort::scope_for_custodies(
                    &fields.policy_context,
                    fields.payer_custodies.iter(),
                )
        {
            return Err(MonetaryCohortError::InvalidEvidence);
        }
        let evidence = Self { fields };
        evidence.transition()?;
        Ok(evidence)
    }
}

impl From<MonetaryFeeEvidence> for MonetaryFeeFields {
    fn from(evidence: MonetaryFeeEvidence) -> Self { evidence.fields }
}

impl MonetaryFeeEvidence {
    pub fn from_plan(
        cohort: &MonetaryCohort,
        plan: &MonetaryCohortPlan,
        policy_context: [u8; 32],
        obligation: u64,
    ) -> Result<Self, MonetaryCohortError> {
        if plan
            .settlement()
            .custody_debit
            .0
            .values()
            .map(|amount| u128::from(*amount))
            .sum::<u128>()
            != u128::from(obligation)
        {
            return Err(MonetaryCohortError::InvalidEvidence);
        }
        let transition = plan.cursor_transition();
        Self::try_from(MonetaryFeeFields {
            policy_version: MONETARY_FEE_POLICY_VERSION,
            policy_context,
            scope: *transition.scope(),
            payer_custodies: cohort
                .payers()
                .iter()
                .map(|payer| *payer.custody())
                .collect(),
            obligation,
            expected_revision: transition.expected().revision(),
            expected_position: transition.expected().position(),
            next_revision: transition.next().revision(),
            next_position: transition.next().position(),
        })
    }

    pub fn fields(&self) -> &MonetaryFeeFields { &self.fields }

    pub fn payer_count(&self) -> NonZeroUsize {
        NonZeroUsize::new(self.fields.payer_custodies.len()).expect("validated monetary cohort")
    }

    pub fn transition(&self) -> Result<MonetaryCursorTransition, MonetaryCohortError> {
        let fields = &self.fields;
        let count = self.payer_count();
        let expected =
            MonetaryCursor::new(fields.expected_revision, fields.expected_position, count)?;
        let next = MonetaryCursor::new(fields.next_revision, fields.next_position, count)?;
        Ok(MonetaryCursorTransition::from_parts(
            fields.scope,
            expected,
            next,
            count,
        )?)
    }

    pub fn write_canonical(&self, bytes: &mut Vec<u8>) {
        let fields = &self.fields;
        bytes.extend_from_slice(&fields.policy_version.to_le_bytes());
        bytes.extend_from_slice(&fields.policy_context);
        bytes.extend_from_slice(&fields.scope);
        bytes.extend_from_slice(&(fields.payer_custodies.len() as u64).to_le_bytes());
        for payer in &fields.payer_custodies {
            bytes.extend_from_slice(payer);
        }
        bytes.extend_from_slice(&fields.obligation.to_le_bytes());
        bytes.extend_from_slice(&fields.expected_revision.to_le_bytes());
        bytes.extend_from_slice(&fields.expected_position.to_le_bytes());
        bytes.extend_from_slice(&fields.next_revision.to_le_bytes());
        bytes.extend_from_slice(&fields.next_position.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn fields(count: usize, revision: i64, position: i64, next_position: i64) -> MonetaryFeeFields {
        let payer_custodies = (0..count)
            .map(|index| {
                let mut custody = [0; 32];
                custody[..8].copy_from_slice(&(index as u64).to_be_bytes());
                custody
            })
            .collect::<Vec<_>>();
        let policy_context = [7; 32];
        MonetaryFeeFields {
            policy_version: MONETARY_FEE_POLICY_VERSION,
            policy_context,
            scope: super::super::cohort::scope_for_custodies(
                &policy_context,
                payer_custodies.iter(),
            ),
            payer_custodies,
            obligation: 1,
            expected_revision: revision,
            expected_position: position,
            next_revision: revision + 1,
            next_position,
        }
    }

    #[test]
    fn fee_evidence_rejects_malformed_and_unbound_fields() {
        let valid = fields(3, 2, 1, 2);
        for mutation in 0..11 {
            let mut changed = valid.clone();
            match mutation {
                0 => changed.policy_version += 1,
                1 => changed.policy_context[0] ^= 1,
                2 => changed.scope[0] ^= 1,
                3 => changed.payer_custodies.clear(),
                4 => changed.payer_custodies.reverse(),
                5 => changed.payer_custodies[1] = changed.payer_custodies[0],
                6 => changed.obligation = 0,
                7 => changed.expected_revision = -1,
                8 => changed.expected_position = 3,
                9 => changed.next_revision += 1,
                10 => changed.next_position = -1,
                _ => unreachable!(),
            }
            assert!(MonetaryFeeEvidence::try_from(changed.clone()).is_err());
            assert!(serde_json::from_value::<MonetaryFeeEvidence>(
                serde_json::to_value(&changed).unwrap()
            )
            .is_err());
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn fee_evidence_roundtrip_preserves_scoped_successor(
            count in 1_usize..=129,
            revision in 0_i64..i64::MAX,
            position_seed in any::<u64>(),
            next_seed in any::<u64>(),
        ) {
            let fields = fields(count, revision, (position_seed % count as u64) as i64, (next_seed % count as u64) as i64);
            let evidence = MonetaryFeeEvidence::try_from(fields.clone()).unwrap();
            let encoded = serde_json::to_vec(&evidence).unwrap();
            let decoded: MonetaryFeeEvidence = serde_json::from_slice(&encoded).unwrap();
            prop_assert_eq!(&evidence, &decoded);
            let transition = decoded.transition().unwrap();
            prop_assert_eq!(transition.scope(), &fields.scope);
            prop_assert_eq!(transition.next().revision(), revision + 1);
            prop_assert!(transition.checked_successor(&fields.scope, transition.next(), evidence.payer_count()).is_err());
            let mut bytes = Vec::new();
            evidence.write_canonical(&mut bytes);
            let mut changed_fields = fields;
            changed_fields.obligation += 1;
            let changed = MonetaryFeeEvidence::try_from(changed_fields).unwrap();
            let mut changed_bytes = Vec::new();
            changed.write_canonical(&mut changed_bytes);
            prop_assert_ne!(bytes, changed_bytes);
        }
    }
}
