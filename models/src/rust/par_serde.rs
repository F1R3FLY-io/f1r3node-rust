use std::fmt;

use serde::de::{self, SeqAccess, Visitor};
use serde::ser::SerializeTuple;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::rhoapi::tagged_continuation::TaggedCont;
use crate::rhoapi::{
    Bundle, Connective, CostSignature, CostSignedTerm, CostStack, Expr, GUnforgeable, If,
    ListParWithRandom, Match, New, Par, Receive, ReceiveBind, SealRecordProto, Send,
    TaggedContinuation, Var,
};

const CONNECTIVE_USED: u8 = 1;
const PAR_EXTENSION: u8 = 2;
const REMAINDER_PRESENT: u8 = 1;
const BIND_EXTENSION: u8 = 2;
const GUARD_PRESENT: u8 = 1;
const SEAL_PRESENT: u8 = 2;
const DATUM_SEALED: u64 = 1 << 63;

struct EmptyBytes;

impl Serialize for EmptyBytes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&[])
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename = "Par")]
struct ParFields {
    sends: Vec<Send>,
    receives: Vec<Receive>,
    news: Vec<New>,
    exprs: Vec<Expr>,
    matches: Vec<Match>,
    unforgeables: Vec<GUnforgeable>,
    bundles: Vec<Bundle>,
    connectives: Vec<Connective>,
    conditionals: Vec<If>,
    #[serde(serialize_with = "crate::rust::serde_helpers::serialize_as_empty_bytes")]
    locally_free: Vec<u8>,
    connective_used: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    cost_signed_terms: Vec<CostSignedTerm>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    cost_stacks: Vec<CostStack>,
}

impl From<ParFields> for Par {
    fn from(f: ParFields) -> Self {
        Par {
            sends: f.sends,
            receives: f.receives,
            news: f.news,
            exprs: f.exprs,
            matches: f.matches,
            unforgeables: f.unforgeables,
            bundles: f.bundles,
            connectives: f.connectives,
            conditionals: f.conditionals,
            locally_free: f.locally_free,
            connective_used: f.connective_used,
            cost_signed_terms: f.cost_signed_terms,
            cost_stacks: f.cost_stacks,
        }
    }
}

impl Serialize for Par {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            return ParFields {
                sends: self.sends.clone(),
                receives: self.receives.clone(),
                news: self.news.clone(),
                exprs: self.exprs.clone(),
                matches: self.matches.clone(),
                unforgeables: self.unforgeables.clone(),
                bundles: self.bundles.clone(),
                connectives: self.connectives.clone(),
                conditionals: self.conditionals.clone(),
                locally_free: self.locally_free.clone(),
                connective_used: self.connective_used,
                cost_signed_terms: self.cost_signed_terms.clone(),
                cost_stacks: self.cost_stacks.clone(),
            }
            .serialize(serializer);
        }
        let extended = !self.cost_signed_terms.is_empty() || !self.cost_stacks.is_empty();
        let mut flags = if self.connective_used {
            CONNECTIVE_USED
        } else {
            0
        };
        if extended {
            flags |= PAR_EXTENSION;
        }
        let mut tuple = serializer.serialize_tuple(if extended { 13 } else { 11 })?;
        tuple.serialize_element(&self.sends)?;
        tuple.serialize_element(&self.receives)?;
        tuple.serialize_element(&self.news)?;
        tuple.serialize_element(&self.exprs)?;
        tuple.serialize_element(&self.matches)?;
        tuple.serialize_element(&self.unforgeables)?;
        tuple.serialize_element(&self.bundles)?;
        tuple.serialize_element(&self.connectives)?;
        tuple.serialize_element(&self.conditionals)?;
        tuple.serialize_element(&EmptyBytes)?;
        tuple.serialize_element(&flags)?;
        if extended {
            tuple.serialize_element(&self.cost_signed_terms)?;
            tuple.serialize_element(&self.cost_stacks)?;
        }
        tuple.end()
    }
}

fn next<'de, A: SeqAccess<'de>, T: Deserialize<'de>>(
    seq: &mut A,
    index: usize,
) -> Result<T, A::Error> {
    seq.next_element()?
        .ok_or_else(|| de::Error::invalid_length(index, &"a complete binary term"))
}

struct BinaryPar;

impl<'de> Visitor<'de> for BinaryPar {
    type Value = Par;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result { f.write_str("a binary Par") }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Par, A::Error> {
        let mut par = Par {
            sends: next(&mut seq, 0)?,
            receives: next(&mut seq, 1)?,
            news: next(&mut seq, 2)?,
            exprs: next(&mut seq, 3)?,
            matches: next(&mut seq, 4)?,
            unforgeables: next(&mut seq, 5)?,
            bundles: next(&mut seq, 6)?,
            connectives: next(&mut seq, 7)?,
            conditionals: next(&mut seq, 8)?,
            locally_free: next(&mut seq, 9)?,
            ..Par::default()
        };
        let flags: u8 = next(&mut seq, 10)?;
        if flags & !(CONNECTIVE_USED | PAR_EXTENSION) != 0 {
            return Err(de::Error::custom(format!("unknown Par flags {flags:#x}")));
        }
        par.connective_used = flags & CONNECTIVE_USED != 0;
        if flags & PAR_EXTENSION != 0 {
            par.cost_signed_terms = next(&mut seq, 11)?;
            par.cost_stacks = next(&mut seq, 12)?;
            if par.cost_signed_terms.is_empty() && par.cost_stacks.is_empty() {
                return Err(de::Error::custom("a Par extension with no cost terms"));
            }
        }
        Ok(par)
    }
}

impl<'de> Deserialize<'de> for Par {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            ParFields::deserialize(deserializer).map(Par::from)
        } else {
            deserializer.deserialize_tuple(13, BinaryPar)
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename = "ReceiveBind")]
struct ReceiveBindFields {
    patterns: Vec<Par>,
    source: Option<Par>,
    remainder: Option<Var>,
    free_count: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cost_signature: Option<CostSignature>,
}

impl Serialize for ReceiveBind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            return ReceiveBindFields {
                patterns: self.patterns.clone(),
                source: self.source.clone(),
                remainder: self.remainder.clone(),
                free_count: self.free_count,
                cost_signature: self.cost_signature.clone(),
            }
            .serialize(serializer);
        }
        let mut tag = 0;
        if self.remainder.is_some() {
            tag |= REMAINDER_PRESENT;
        }
        if self.cost_signature.is_some() {
            tag |= BIND_EXTENSION;
        }
        let mut tuple = serializer.serialize_tuple(6)?;
        tuple.serialize_element(&self.patterns)?;
        tuple.serialize_element(&self.source)?;
        tuple.serialize_element(&tag)?;
        if let Some(remainder) = &self.remainder {
            tuple.serialize_element(remainder)?;
        }
        tuple.serialize_element(&self.free_count)?;
        if let Some(signature) = &self.cost_signature {
            tuple.serialize_element(signature)?;
        }
        tuple.end()
    }
}

struct BinaryReceiveBind;

impl<'de> Visitor<'de> for BinaryReceiveBind {
    type Value = ReceiveBind;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a binary ReceiveBind")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<ReceiveBind, A::Error> {
        let patterns = next(&mut seq, 0)?;
        let source = next(&mut seq, 1)?;
        let tag: u8 = next(&mut seq, 2)?;
        if tag & !(REMAINDER_PRESENT | BIND_EXTENSION) != 0 {
            return Err(de::Error::custom(format!(
                "unknown ReceiveBind tag {tag:#x}"
            )));
        }
        let remainder = if tag & REMAINDER_PRESENT != 0 {
            Some(next(&mut seq, 3)?)
        } else {
            None
        };
        let free_count = next(&mut seq, 4)?;
        let cost_signature = if tag & BIND_EXTENSION != 0 {
            Some(next(&mut seq, 5)?)
        } else {
            None
        };
        Ok(ReceiveBind {
            patterns,
            source,
            remainder,
            free_count,
            cost_signature,
        })
    }
}

impl<'de> Deserialize<'de> for ReceiveBind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let f = ReceiveBindFields::deserialize(deserializer)?;
            Ok(ReceiveBind {
                patterns: f.patterns,
                source: f.source,
                remainder: f.remainder,
                free_count: f.free_count,
                cost_signature: f.cost_signature,
            })
        } else {
            deserializer.deserialize_tuple(6, BinaryReceiveBind)
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename = "TaggedContinuation")]
struct TaggedContinuationFields {
    guard: Option<Par>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    seal: Option<SealRecordProto>,
    tagged_cont: Option<TaggedCont>,
}

impl Serialize for TaggedContinuation {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            return TaggedContinuationFields {
                guard: self.guard.clone(),
                seal: self.seal.clone(),
                tagged_cont: self.tagged_cont.clone(),
            }
            .serialize(serializer);
        }
        let mut tag = 0;
        if self.guard.is_some() {
            tag |= GUARD_PRESENT;
        }
        if self.seal.is_some() {
            tag |= SEAL_PRESENT;
        }
        let mut tuple = serializer.serialize_tuple(4)?;
        tuple.serialize_element(&tag)?;
        if let Some(guard) = &self.guard {
            tuple.serialize_element(guard)?;
        }
        if let Some(seal) = &self.seal {
            tuple.serialize_element(seal)?;
        }
        tuple.serialize_element(&self.tagged_cont)?;
        tuple.end()
    }
}

struct BinaryTaggedContinuation;

impl<'de> Visitor<'de> for BinaryTaggedContinuation {
    type Value = TaggedContinuation;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a binary TaggedContinuation")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<TaggedContinuation, A::Error> {
        let tag: u8 = next(&mut seq, 0)?;
        if tag & !(GUARD_PRESENT | SEAL_PRESENT) != 0 {
            return Err(de::Error::custom(format!(
                "unknown TaggedContinuation tag {tag:#x}"
            )));
        }
        let guard = if tag & GUARD_PRESENT != 0 {
            Some(next(&mut seq, 1)?)
        } else {
            None
        };
        let seal = if tag & SEAL_PRESENT != 0 {
            Some(next(&mut seq, 2)?)
        } else {
            None
        };
        let tagged_cont = next(&mut seq, 3)?;
        Ok(TaggedContinuation {
            tagged_cont,
            guard,
            seal,
        })
    }
}

impl<'de> Deserialize<'de> for TaggedContinuation {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let f = TaggedContinuationFields::deserialize(deserializer)?;
            Ok(TaggedContinuation {
                tagged_cont: f.tagged_cont,
                guard: f.guard,
                seal: f.seal,
            })
        } else {
            deserializer.deserialize_tuple(4, BinaryTaggedContinuation)
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename = "ListParWithRandom")]
struct ListParWithRandomFields {
    pars: Vec<Par>,
    random_state: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    seal: Option<SealRecordProto>,
}

impl Serialize for ListParWithRandom {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            return ListParWithRandomFields {
                pars: self.pars.clone(),
                random_state: self.random_state.clone(),
                seal: self.seal.clone(),
            }
            .serialize(serializer);
        }
        let mut header = self.random_state.len() as u64;
        if header & DATUM_SEALED != 0 {
            return Err(serde::ser::Error::custom(
                "a random state too long to encode",
            ));
        }
        if self.seal.is_some() {
            header |= DATUM_SEALED;
        }
        let mut tuple = serializer.serialize_tuple(3 + self.random_state.len())?;
        tuple.serialize_element(&self.pars)?;
        tuple.serialize_element(&header)?;
        for byte in &self.random_state {
            tuple.serialize_element(byte)?;
        }
        if let Some(seal) = &self.seal {
            tuple.serialize_element(seal)?;
        }
        tuple.end()
    }
}

struct BinaryListParWithRandom;

impl<'de> Visitor<'de> for BinaryListParWithRandom {
    type Value = ListParWithRandom;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a binary ListParWithRandom")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<ListParWithRandom, A::Error> {
        let pars = next(&mut seq, 0)?;
        let header: u64 = next(&mut seq, 1)?;
        let len = header & !DATUM_SEALED;
        let mut random_state = Vec::with_capacity(len.min(1024) as usize);
        for i in 0..len {
            random_state.push(next(&mut seq, 2 + i as usize)?);
        }
        let seal = if header & DATUM_SEALED != 0 {
            Some(next(&mut seq, 2 + len as usize)?)
        } else {
            None
        };
        Ok(ListParWithRandom {
            pars,
            random_state,
            seal,
        })
    }
}

impl<'de> Deserialize<'de> for ListParWithRandom {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let f = ListParWithRandomFields::deserialize(deserializer)?;
            Ok(ListParWithRandom {
                pars: f.pars,
                random_state: f.random_state,
                seal: f.seal,
            })
        } else {
            deserializer.deserialize_tuple(usize::MAX, BinaryListParWithRandom)
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::rhoapi::cost_signature::Value;
    use crate::rust::test_utils::test_utils::generate_par;

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct LegacyPar {
        sends: Vec<Send>,
        receives: Vec<Receive>,
        news: Vec<New>,
        exprs: Vec<Expr>,
        matches: Vec<Match>,
        unforgeables: Vec<GUnforgeable>,
        bundles: Vec<Bundle>,
        connectives: Vec<Connective>,
        conditionals: Vec<If>,
        #[serde(serialize_with = "crate::rust::serde_helpers::serialize_as_empty_bytes")]
        locally_free: Vec<u8>,
        connective_used: bool,
    }

    #[derive(Serialize)]
    struct LegacyTaggedContinuation {
        guard: Option<Par>,
        tagged_cont: Option<TaggedCont>,
    }

    fn small(n: i64) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(crate::rhoapi::expr::ExprInstance::GInt(n)),
        }])
    }

    fn continuation(guard: Option<Par>, seal: Option<SealRecordProto>) -> TaggedContinuation {
        TaggedContinuation {
            tagged_cont: Some(TaggedCont::ParBody(crate::rhoapi::ParWithRandom {
                body: Some(small(1)),
                random_state: vec![3; 8],
            })),
            guard,
            seal,
        }
    }

    #[test]
    fn a_continuation_without_a_seal_keeps_the_legacy_bytes() {
        for guard in [None, Some(small(2))] {
            let c = continuation(guard.clone(), None);
            let legacy = LegacyTaggedContinuation {
                tagged_cont: c.tagged_cont.clone(),
                guard,
            };
            assert_eq!(
                bincode::serialize(&c).unwrap(),
                bincode::serialize(&legacy).unwrap()
            );
            let back: TaggedContinuation =
                bincode::deserialize(&bincode::serialize(&c).unwrap()).unwrap();
            assert_eq!(back, c);
        }
    }

    #[test]
    fn a_sealed_continuation_round_trips_and_differs_from_the_unsealed_one() {
        let seal = SealRecordProto {
            owner: vec![2; 33],
            kind: 1,
            origin: Some(crate::rhoapi::SealOrigin {
                envelope: vec![9; 64],
                signers: vec![vec![1; 33], vec![2; 33]],
                phlo_price: 7,
            }),
        };
        for guard in [None, Some(small(2))] {
            let sealed = continuation(guard.clone(), Some(seal.clone()));
            let bytes = bincode::serialize(&sealed).unwrap();
            let back: TaggedContinuation = bincode::deserialize(&bytes).unwrap();
            assert_eq!(back, sealed);
            assert_ne!(sealed, continuation(guard.clone(), None));
            assert_ne!(
                bytes,
                bincode::serialize(&continuation(guard, None)).unwrap()
            );
        }
        let mut bytes = bincode::serialize(&continuation(None, None)).unwrap();
        bytes[0] = 4;
        assert!(bincode::deserialize::<TaggedContinuation>(&bytes).is_err());
    }

    #[derive(Serialize)]
    struct LegacyListParWithRandom {
        pars: Vec<Par>,
        random_state: Vec<u8>,
    }

    fn datum(seal: Option<SealRecordProto>) -> ListParWithRandom {
        ListParWithRandom {
            pars: vec![small(1), small(2)],
            random_state: vec![5; 128],
            seal,
        }
    }

    fn a_seal() -> SealRecordProto {
        SealRecordProto {
            owner: vec![2; 33],
            kind: 1,
            origin: Some(crate::rhoapi::SealOrigin {
                envelope: vec![9; 64],
                signers: vec![vec![1; 33]],
                phlo_price: 7,
            }),
        }
    }

    #[test]
    fn a_datum_without_a_seal_keeps_the_legacy_bytes() {
        use bincode::Options;
        let d = datum(None);
        let legacy = LegacyListParWithRandom {
            pars: d.pars.clone(),
            random_state: d.random_state.clone(),
        };
        assert_eq!(
            bincode::serialize(&d).unwrap(),
            bincode::serialize(&legacy).unwrap()
        );
        let varint = bincode::DefaultOptions::new();
        assert_eq!(
            varint.serialize(&d).unwrap(),
            varint.serialize(&legacy).unwrap()
        );
        let followed = (d.clone(), 7u32);
        let back: (ListParWithRandom, u32) =
            bincode::deserialize(&bincode::serialize(&followed).unwrap()).unwrap();
        assert_eq!(back, followed);
        let back: (ListParWithRandom, u32) = varint
            .deserialize(&varint.serialize(&followed).unwrap())
            .unwrap();
        assert_eq!(back, followed);
    }

    #[test]
    fn a_sealed_datum_round_trips_and_differs_from_the_unsealed_one() {
        use bincode::Options;
        let sealed = datum(Some(a_seal()));
        let followed = (sealed.clone(), 7u32);
        let back: (ListParWithRandom, u32) =
            bincode::deserialize(&bincode::serialize(&followed).unwrap()).unwrap();
        assert_eq!(back, followed);
        let varint = bincode::DefaultOptions::new();
        let back: (ListParWithRandom, u32) = varint
            .deserialize(&varint.serialize(&followed).unwrap())
            .unwrap();
        assert_eq!(back, followed);
        assert_ne!(sealed, datum(None));
        assert_ne!(
            bincode::serialize(&sealed).unwrap(),
            bincode::serialize(&datum(None)).unwrap()
        );
        let json: ListParWithRandom =
            serde_json::from_str(&serde_json::to_string(&sealed).unwrap()).unwrap();
        assert_eq!(json, sealed);
        let json = serde_json::to_string(&datum(None)).unwrap();
        assert!(!json.contains("seal"));
    }

    #[derive(Serialize)]
    struct LegacyReceiveBind {
        patterns: Vec<Par>,
        source: Option<Par>,
        remainder: Option<Var>,
        free_count: i32,
    }

    fn legacy(par: &Par) -> LegacyPar {
        LegacyPar {
            sends: par.sends.clone(),
            receives: par.receives.clone(),
            news: par.news.clone(),
            exprs: par.exprs.clone(),
            matches: par.matches.clone(),
            unforgeables: par.unforgeables.clone(),
            bundles: par.bundles.clone(),
            connectives: par.connectives.clone(),
            conditionals: par.conditionals.clone(),
            locally_free: par.locally_free.clone(),
            connective_used: par.connective_used,
        }
    }

    fn ground(b: u8) -> CostSignature {
        CostSignature {
            value: Some(Value::Ground(vec![b])),
        }
    }

    fn without_locally_free(mut par: Par) -> Par {
        par.locally_free.clear();
        par
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn a_par_without_cost_terms_encodes_as_before(par in generate_par(3)) {
            let new = bincode::serialize(&par).unwrap();
            prop_assert_eq!(&new, &bincode::serialize(&legacy(&par)).unwrap());
            let back: Par = bincode::deserialize(&new).unwrap();
            prop_assert_eq!(back, without_locally_free(par.clone()));
            let json: Par = serde_json::from_str(&serde_json::to_string(&par).unwrap()).unwrap();
            prop_assert_eq!(json, without_locally_free(par));
        }
    }

    #[test]
    fn a_receive_bind_without_a_signature_encodes_as_before() {
        for remainder in [None, Some(Var::default())] {
            let bind = ReceiveBind {
                patterns: vec![Par::default()],
                source: Some(Par::default()),
                remainder: remainder.clone(),
                free_count: 3,
                cost_signature: None,
            };
            let legacy = LegacyReceiveBind {
                patterns: vec![Par::default()],
                source: Some(Par::default()),
                remainder,
                free_count: 3,
            };
            let bytes = bincode::serialize(&bind).unwrap();
            assert_eq!(bytes, bincode::serialize(&legacy).unwrap());
            assert_eq!(bincode::deserialize::<ReceiveBind>(&bytes).unwrap(), bind);
        }
    }

    #[test]
    fn cost_terms_round_trip_and_change_the_encoding() {
        let par = Par {
            cost_signed_terms: vec![CostSignedTerm {
                body: Some(Par::default()),
                signature: Some(ground(1)),
            }],
            cost_stacks: vec![CostStack {
                cells: vec![ground(2), ground(3)],
            }],
            connective_used: true,
            ..Par::default()
        };
        let bytes = bincode::serialize(&par).unwrap();
        assert_ne!(bytes, bincode::serialize(&legacy(&par)).unwrap());
        assert_eq!(bincode::deserialize::<Par>(&bytes).unwrap(), par);
        let json = serde_json::to_string(&par).unwrap();
        assert_eq!(serde_json::from_str::<Par>(&json).unwrap(), par);

        let bind = ReceiveBind {
            patterns: vec![par.clone()],
            source: Some(Par::default()),
            remainder: Some(Var::default()),
            free_count: 1,
            cost_signature: Some(ground(4)),
        };
        let bytes = bincode::serialize(&bind).unwrap();
        assert_eq!(bincode::deserialize::<ReceiveBind>(&bytes).unwrap(), bind);
    }

    #[test]
    fn an_empty_extension_and_unknown_flags_are_rejected() {
        let mut bytes = bincode::serialize(&Par::default()).unwrap();
        let flags = bytes.len() - 1;
        bytes[flags] = PAR_EXTENSION;
        bytes.extend(bincode::serialize(&Vec::<CostSignedTerm>::new()).unwrap());
        bytes.extend(bincode::serialize(&Vec::<CostStack>::new()).unwrap());
        assert!(bincode::deserialize::<Par>(&bytes).is_err());

        let mut bytes = bincode::serialize(&Par::default()).unwrap();
        bytes[flags] = 4;
        assert!(bincode::deserialize::<Par>(&bytes).is_err());
    }
}

