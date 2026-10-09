//! The identity of a theory (P7 design v3, TH-2, and decision 74).
//!
//! An identity names a checker by its content only, never by a handle, a token or an epoch, so a
//! node that restarts, a validator that replays and a node that attaches later resolve it alike.
//! The identity has two canonical forms:
//!
//! * The bytes of [`TheoryIdentity::to_bytes`], which the config of a space holds in one
//!   `GByteArray` (decision 74). Every operation reads the config, and one `Par` there costs far
//!   less to read than a map of eight entries.
//! * The `Par` map of [`TheoryIdentity::to_par`], which the `config` reply of a handle shows:
//!
//! ```text
//! {"version": 1, "languageFingerprint": "rrs:builtin", "category": "Proc",
//!  "categoryName": "Proc", "theoremId": "Nat", "checkerAbi": "rrs:structural:v1",
//!  "limitProfile": {"workUnits": 1, "evidenceBytes": 0}, "args": []}
//! ```
//!
//! Each decoder accepts exactly the image of its encoder.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Expr, Par};
use prost::Message;

use super::TheoryUsage;
use crate::rust::interpreter::spaces::record::{expr_par, sole_expr};
use crate::rust::interpreter::spaces::types::config::{map_entries, map_par, string_par};

/// The version of the identity map.
pub const IDENTITY_VERSION: i64 = 1;

/// The language fingerprint of the structural builtins.
pub const BUILTIN_LANGUAGE: &str = "rrs:builtin";

/// The category of the structural builtins: they admit processes.
pub const BUILTIN_CATEGORY: &str = "Proc";

/// The checker ABI of the structural builtins.
pub const STRUCTURAL_ABI: &str = "rrs:structural:v1";

/// The limit profile of the structural builtins: one unit of work and no evidence.
pub const STRUCTURAL_LIMITS: TheoryUsage = TheoryUsage::new(1, 0);

const VERSION_KEY: &str = "version";
const LANGUAGE_KEY: &str = "languageFingerprint";
const CATEGORY_KEY: &str = "category";
const CATEGORY_NAME_KEY: &str = "categoryName";
const THEOREM_KEY: &str = "theoremId";
const ABI_KEY: &str = "checkerAbi";
const LIMITS_KEY: &str = "limitProfile";
const ARGS_KEY: &str = "args";
const WORK_KEY: &str = "workUnits";
const EVIDENCE_KEY: &str = "evidenceBytes";

/// The number of entries of an identity map.
const IDENTITY_ENTRIES: usize = 8;

/// The identity of a theory: the content that names its checker (TH-2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TheoryIdentity {
    /// The version of the identity map.
    pub version: i64,
    /// The fingerprint of the language of the theory.
    pub language_fingerprint: String,
    /// The category of the terms that the theory admits.
    pub category: String,
    /// The name of that category.
    pub category_name: String,
    /// The id of the theorem that the checker decides.
    pub theorem_id: String,
    /// The ABI of the checker.
    pub checker_abi: String,
    /// The limits of one check.
    pub limit_profile: TheoryUsage,
    /// The arguments of the theory.
    pub args: Vec<Par>,
}

impl TheoryIdentity {
    /// The identity of the structural builtin `name` (TH-2).
    pub fn builtin(name: &str) -> Self {
        Self {
            version: IDENTITY_VERSION,
            language_fingerprint: BUILTIN_LANGUAGE.to_string(),
            category: BUILTIN_CATEGORY.to_string(),
            category_name: BUILTIN_CATEGORY.to_string(),
            theorem_id: name.to_string(),
            checker_abi: STRUCTURAL_ABI.to_string(),
            limit_profile: STRUCTURAL_LIMITS,
            args: Vec::new(),
        }
    }

    /// The canonical identity map.
    pub fn to_par(&self) -> Par {
        let limits = map_par(vec![
            (string_par(WORK_KEY), int_par(self.limit_profile.work_units)),
            (
                string_par(EVIDENCE_KEY),
                int_par(self.limit_profile.evidence_bytes),
            ),
        ]);
        map_par(vec![
            (
                string_par(VERSION_KEY),
                expr_par(ExprInstance::GInt(self.version)),
            ),
            (
                string_par(LANGUAGE_KEY),
                string_par(&self.language_fingerprint),
            ),
            (string_par(CATEGORY_KEY), string_par(&self.category)),
            (
                string_par(CATEGORY_NAME_KEY),
                string_par(&self.category_name),
            ),
            (string_par(THEOREM_KEY), string_par(&self.theorem_id)),
            (string_par(ABI_KEY), string_par(&self.checker_abi)),
            (string_par(LIMITS_KEY), limits),
            (string_par(ARGS_KEY), list_par(self.args.clone())),
        ])
    }

    /// The canonical bytes of the identity (decision 74): its fields in their order of
    /// declaration. A number is big endian: `version` as an `i64`, and the two limits as `u64`
    /// values. A string is its length as a big-endian `u32`, then its UTF-8 bytes. The args are
    /// their count as a big-endian `u32`, then each arg as its length and its prost bytes. The
    /// buffer is allocated once, with its exact length.
    pub fn to_bytes(&self) -> Vec<u8> {
        let strings = [
            &self.language_fingerprint,
            &self.category,
            &self.category_name,
            &self.theorem_id,
            &self.checker_abi,
        ];
        let length = size_of::<i64>()
            + strings
                .iter()
                .map(|value| size_of::<u32>() + value.len())
                .sum::<usize>()
            + 2 * size_of::<u64>()
            + size_of::<u32>()
            + self
                .args
                .iter()
                .map(|arg| size_of::<u32>() + arg.encoded_len())
                .sum::<usize>();
        let mut bytes = Vec::with_capacity(length);
        bytes.extend_from_slice(&self.version.to_be_bytes());
        for value in strings {
            push_length(&mut bytes, value.len());
            bytes.extend_from_slice(value.as_bytes());
        }
        bytes.extend_from_slice(&self.limit_profile.work_units.to_be_bytes());
        bytes.extend_from_slice(&self.limit_profile.evidence_bytes.to_be_bytes());
        push_length(&mut bytes, self.args.len());
        for arg in &self.args {
            let encoded = arg.encode_to_vec();
            push_length(&mut bytes, encoded.len());
            bytes.extend_from_slice(&encoded);
        }
        bytes
    }

    /// Decodes canonical identity bytes. Anything but the exact image of
    /// [`TheoryIdentity::to_bytes`] is `None`: a short or a long input, a string that is not
    /// UTF-8, an arg that prost cannot decode, or an arg whose bytes are not the bytes of its
    /// decoded `Par`.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let mut reader = Reader { rest: bytes };
        let version = i64::from_be_bytes(reader.array()?);
        let language_fingerprint = reader.string()?;
        let category = reader.string()?;
        let category_name = reader.string()?;
        let theorem_id = reader.string()?;
        let checker_abi = reader.string()?;
        let limit_profile = TheoryUsage::new(
            u64::from_be_bytes(reader.array()?),
            u64::from_be_bytes(reader.array()?),
        );
        let count = reader.length()?;
        // Each arg takes at least the four bytes of its length, which bounds the allocation.
        let mut args = Vec::with_capacity(count.min(reader.rest.len() / size_of::<u32>()));
        for _ in 0..count {
            let length = reader.length()?;
            args.push(Par::decode(reader.take(length)?).ok()?);
        }
        let identity = TheoryIdentity {
            version,
            language_fingerprint,
            category,
            category_name,
            theorem_id,
            checker_abi,
            limit_profile,
            args,
        };
        match reader.rest.is_empty() && identity.to_bytes() == bytes {
            true => Some(identity),
            false => None,
        }
    }

    /// Decodes an identity map. Anything but the exact image of [`TheoryIdentity::to_par`] is
    /// `None`: another key set, a value of another kind, a negative limit, or another entry
    /// order or bit flag.
    pub fn from_par(map: &Par) -> Option<Self> {
        let entries = map_entries(map)?;
        if entries.len() != IDENTITY_ENTRIES {
            return None;
        }
        let entry = |key: &str| entries.get(key).copied();
        let limits = map_entries(entry(LIMITS_KEY)?)?;
        if limits.len() != 2 {
            return None;
        }
        let identity = TheoryIdentity {
            version: int_of(entry(VERSION_KEY)?)?,
            language_fingerprint: string_of(entry(LANGUAGE_KEY)?)?.to_string(),
            category: string_of(entry(CATEGORY_KEY)?)?.to_string(),
            category_name: string_of(entry(CATEGORY_NAME_KEY)?)?.to_string(),
            theorem_id: string_of(entry(THEOREM_KEY)?)?.to_string(),
            checker_abi: string_of(entry(ABI_KEY)?)?.to_string(),
            limit_profile: TheoryUsage::new(
                limit_of(limits.get(WORK_KEY).copied()?)?,
                limit_of(limits.get(EVIDENCE_KEY).copied()?)?,
            ),
            args: list_of(entry(ARGS_KEY)?)?.to_vec(),
        };
        match identity.to_par() == *map {
            true => Some(identity),
            false => None,
        }
    }
}

/// Appends `length` as a big-endian `u32`. A length above `u32::MAX` cannot occur: a field of
/// an identity is a name or a small `Par`, far below 4 GiB, so the encoder saturates instead of
/// panicking, and the decoder then rejects the bytes.
fn push_length(bytes: &mut Vec<u8>, length: usize) {
    let length = u32::try_from(length).unwrap_or(u32::MAX);
    bytes.extend_from_slice(&length.to_be_bytes());
}

/// A cursor over identity bytes.
struct Reader<'a> {
    rest: &'a [u8],
}

impl<'a> Reader<'a> {
    /// The next `length` bytes.
    fn take(&mut self, length: usize) -> Option<&'a [u8]> {
        match self.rest.split_at_checked(length) {
            Some((head, tail)) => {
                self.rest = tail;
                Some(head)
            }
            None => None,
        }
    }

    /// The next `N` bytes as an array.
    fn array<const N: usize>(&mut self) -> Option<[u8; N]> { self.take(N)?.try_into().ok() }

    /// The next big-endian `u32` as a length.
    fn length(&mut self) -> Option<usize> {
        usize::try_from(u32::from_be_bytes(self.array()?)).ok()
    }

    /// The next string: its length and its UTF-8 bytes.
    fn string(&mut self) -> Option<String> {
        let length = self.length()?;
        String::from_utf8(self.take(length)?.to_vec()).ok()
    }
}

/// A limit as a `GInt`, or as a non-negative `GBigInt` (two's complement, big endian) when it
/// exceeds `i64::MAX`. So the map holds every `u64` limit without loss, and the decoder keeps one
/// canonical form for each limit.
fn int_par(value: u64) -> Par {
    match i64::try_from(value) {
        Ok(small) => expr_par(ExprInstance::GInt(small)),
        Err(_) => {
            let mut bytes = Vec::with_capacity(9);
            bytes.push(0);
            bytes.extend_from_slice(&value.to_be_bytes());
            expr_par(ExprInstance::GBigInt(bytes))
        }
    }
}

/// The limit of `par`: a `GInt` of at least 0, or the `GBigInt` form of [`int_par`]. The
/// decoder compares the re-encoded map, so only the canonical form of each limit decodes.
fn limit_of(par: &Par) -> Option<u64> {
    match (sole_expr(par), par.connective_used) {
        (Some(ExprInstance::GInt(value)), false) => u64::try_from(*value).ok(),
        (Some(ExprInstance::GBigInt(bytes)), false) => match bytes.as_slice() {
            [0, rest @ ..] if rest.len() == 8 => {
                let mut word = [0u8; 8];
                word.copy_from_slice(rest);
                Some(u64::from_be_bytes(word))
            }
            _ => None,
        },
        _ => None,
    }
}

fn list_par(ps: Vec<Par>) -> Par {
    let list = EList {
        ps,
        locally_free: Vec::new(),
        connective_used: false,
        remainder: None,
    };
    let mut par = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EListBody(list)),
    }]);
    par.connective_used = false;
    par
}

fn int_of(par: &Par) -> Option<i64> {
    match (sole_expr(par), par.connective_used) {
        (Some(ExprInstance::GInt(value)), false) => Some(*value),
        _ => None,
    }
}

fn string_of(par: &Par) -> Option<&str> {
    match (sole_expr(par), par.connective_used) {
        (Some(ExprInstance::GString(value)), false) => Some(value),
        _ => None,
    }
}

fn list_of(par: &Par) -> Option<&[Par]> {
    match (sole_expr(par), par.connective_used) {
        (
            Some(ExprInstance::EListBody(EList {
                ps,
                remainder: None,
                ..
            })),
            false,
        ) => Some(ps),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use models::rust::utils::{new_gint_par, new_gstring_par};

    use super::*;

    fn with_entry(identity: &TheoryIdentity, key: &str, value: Par) -> Par {
        let map = identity.to_par();
        let entries = map_entries(&map).expect("an identity map");
        let mut pairs: Vec<(Par, Par)> = entries
            .into_iter()
            .map(|(name, entry)| {
                let value = match name == key {
                    true => value.clone(),
                    false => entry.clone(),
                };
                (string_par(name), value)
            })
            .collect();
        if !pairs.iter().any(|(name, _)| *name == string_par(key)) {
            pairs.push((string_par(key), value));
        }
        map_par(pairs)
    }

    #[test]
    fn theory_identity_roundtrips_and_holds_content_only() {
        for name in ["Nat", "Int", "String", "Bool", "Any"] {
            let identity = TheoryIdentity::builtin(name);
            assert_eq!(
                TheoryIdentity::from_par(&identity.to_par()),
                Some(identity.clone())
            );
            let map = identity.to_par();
            let entries = map_entries(&map).expect("an identity map");
            assert_eq!(entries.keys().copied().collect::<Vec<_>>(), vec![
                ARGS_KEY,
                CATEGORY_KEY,
                CATEGORY_NAME_KEY,
                ABI_KEY,
                LANGUAGE_KEY,
                LIMITS_KEY,
                THEOREM_KEY,
                VERSION_KEY
            ]);
            // The map holds no unforgeable name: no handle, no token and no epoch.
            assert!(!format!("{map:?}").contains("GPrivate"), "{name}");
        }
        let with_args = TheoryIdentity {
            args: vec![new_gint_par(3, Vec::new(), false)],
            ..TheoryIdentity::builtin("Nat")
        };
        assert_eq!(
            TheoryIdentity::from_par(&with_args.to_par()),
            Some(with_args)
        );
        for work in [0, 1, i64::MAX as u64, i64::MAX as u64 + 1, u64::MAX] {
            let limits = TheoryIdentity {
                limit_profile: TheoryUsage::new(work, work),
                ..TheoryIdentity::builtin("Any")
            };
            assert_eq!(
                TheoryIdentity::from_par(&limits.to_par()),
                Some(limits),
                "{work}"
            );
        }
        // A limit that fits a GInt has only the GInt form.
        let big_one = map_par(vec![
            (
                string_par(WORK_KEY),
                expr_par(ExprInstance::GBigInt(vec![0, 0, 0, 0, 0, 0, 0, 0, 1])),
            ),
            (string_par(EVIDENCE_KEY), new_gint_par(0, Vec::new(), false)),
        ]);
        assert_eq!(
            TheoryIdentity::from_par(&with_entry(
                &TheoryIdentity::builtin("Nat"),
                LIMITS_KEY,
                big_one
            )),
            None
        );
    }

    #[test]
    fn theory_identity_decoder_accepts_exactly_the_canonical_image() {
        let identity = TheoryIdentity::builtin("Nat");
        assert_eq!(TheoryIdentity::from_par(&Par::default()), None);
        assert_eq!(
            TheoryIdentity::from_par(&new_gstring_par("Nat".to_string(), Vec::new(), false)),
            None
        );
        for (key, value) in [
            (
                VERSION_KEY,
                new_gstring_par("1".to_string(), Vec::new(), false),
            ),
            (THEOREM_KEY, new_gint_par(1, Vec::new(), false)),
            (ABI_KEY, Par::default()),
            (ARGS_KEY, new_gint_par(0, Vec::new(), false)),
            (LIMITS_KEY, new_gint_par(1, Vec::new(), false)),
            (
                LIMITS_KEY,
                map_par(vec![
                    (string_par(WORK_KEY), new_gint_par(-1, Vec::new(), false)),
                    (string_par(EVIDENCE_KEY), new_gint_par(0, Vec::new(), false)),
                ]),
            ),
            ("extra", new_gint_par(0, Vec::new(), false)),
        ] {
            assert_eq!(
                TheoryIdentity::from_par(&with_entry(&identity, key, value)),
                None,
                "{key}"
            );
        }
        let flagged = Par {
            connective_used: true,
            ..identity.to_par()
        };
        assert_eq!(TheoryIdentity::from_par(&flagged), None);
    }

    /// Decision 74: the bytes of an identity decode to the identity, for every builtin, with
    /// args and with every limit. The decoder accepts nothing else: no short or long input, no
    /// string that is not UTF-8, no count beyond the input and no arg in a form that its encoder
    /// does not give.
    #[test]
    fn theory_identity_bytes_roundtrip_and_reject_every_other_input() {
        let mut identities: Vec<TheoryIdentity> = ["Nat", "Int", "String", "Bool", "Any"]
            .into_iter()
            .map(TheoryIdentity::builtin)
            .collect();
        identities.push(TheoryIdentity {
            args: vec![
                new_gint_par(3, Vec::new(), false),
                new_gstring_par("x".to_string(), Vec::new(), false),
                Par::default(),
            ],
            ..TheoryIdentity::builtin("Nat")
        });
        for work in [0, 1, u64::MAX] {
            identities.push(TheoryIdentity {
                version: i64::MIN,
                limit_profile: TheoryUsage::new(work, u64::MAX - work),
                ..TheoryIdentity::builtin("Any")
            });
        }
        identities.push(TheoryIdentity {
            theorem_id: "Théorie".to_string(),
            ..TheoryIdentity::builtin("")
        });
        for identity in &identities {
            let bytes = identity.to_bytes();
            assert_eq!(TheoryIdentity::from_bytes(&bytes), Some(identity.clone()));
            for cut in 0..bytes.len() {
                assert_eq!(TheoryIdentity::from_bytes(&bytes[..cut]), None, "cut {cut}");
            }
            let mut longer = bytes.clone();
            longer.push(0);
            assert_eq!(TheoryIdentity::from_bytes(&longer), None, "a trailing byte");
        }
        let nat = TheoryIdentity::builtin("Nat").to_bytes();
        assert_eq!(nat.len(), 8 + 4 * 5 + 11 + 4 + 4 + 3 + 17 + 8 + 8 + 4);
        // The theorem id "Nat" starts after the version, three strings and its own length.
        let theorem_at = 8 + (4 + 11) + (4 + 4) + (4 + 4) + 4;
        assert_eq!(&nat[theorem_at..theorem_at + 3], b"Nat");
        let mut not_utf8 = nat.clone();
        not_utf8[theorem_at] = 0xff;
        assert_eq!(
            TheoryIdentity::from_bytes(&not_utf8),
            None,
            "a string that is not UTF-8"
        );
        let mut too_many = nat.clone();
        let count_at = too_many.len() - 4;
        too_many[count_at..].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(
            TheoryIdentity::from_bytes(&too_many),
            None,
            "a count beyond the input"
        );
        // An arg in a prost form that its encoder does not give: an empty `Par` with the unknown
        // field 1000. Prost skips the field, so the decoded arg encodes to other bytes.
        let unknown_field = [0xc0, 0x3e, 0x00];
        assert_eq!(
            Par::decode(&unknown_field[..])
                .map(|par| par.encode_to_vec())
                .ok(),
            Some(Vec::new())
        );
        let mut stray = nat.clone();
        stray.truncate(count_at);
        stray.extend_from_slice(&1u32.to_be_bytes());
        stray.extend_from_slice(&3u32.to_be_bytes());
        stray.extend_from_slice(&unknown_field);
        assert_eq!(TheoryIdentity::from_bytes(&stray), None, "a stray arg form");
        let mut canonical = nat.clone();
        canonical.truncate(count_at);
        canonical.extend_from_slice(&1u32.to_be_bytes());
        canonical.extend_from_slice(&0u32.to_be_bytes());
        assert_eq!(
            TheoryIdentity::from_bytes(&canonical),
            Some(TheoryIdentity {
                args: vec![Par::default()],
                ..TheoryIdentity::builtin("Nat")
            }),
            "the canonical form of that arg"
        );
        let mut garbage = nat;
        garbage.truncate(count_at);
        garbage.extend_from_slice(&1u32.to_be_bytes());
        garbage.extend_from_slice(&1u32.to_be_bytes());
        garbage.push(0xff);
        assert_eq!(
            TheoryIdentity::from_bytes(&garbage),
            None,
            "an arg that prost refuses"
        );
    }
}
