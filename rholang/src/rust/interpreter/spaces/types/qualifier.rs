//! The qualifier of a native space: the persistence and the concurrency of its channels (P4
//! design B.8 ports the legacy type as it is).
//!
//! The config record of a space names its qualifier (I-17). The channels of `temp` and `seq`
//! spaces live for one deploy (I-4, P7). The Seq rules need the job order of native execution
//! (P7, design v3 QT-5). Spaces run only in native execution, and outside it every space entry
//! point fails with `RuntimeUnavailable` (decision 90, C6).

use std::fmt;

/// The qualifier of a space.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Default,
    serde::Serialize,
    serde::Deserialize
)]
pub enum SpaceQualifier {
    /// Persistent channels with concurrent access. The channels are consensus state.
    #[default]
    Default,
    /// Channels with concurrent access that live for one deploy.
    Temp,
    /// Channels with sequential access that live for one deploy. Their names cannot be sent.
    Seq,
}

impl SpaceQualifier {
    /// True when the names of the space can be sent to other processes. `seq` names cannot.
    pub fn is_mobile(&self) -> bool { !matches!(self, SpaceQualifier::Seq) }

    /// True when the channels of the space are consensus state.
    pub fn is_persistent(&self) -> bool { matches!(self, SpaceQualifier::Default) }

    /// True when parallel processes can use the space at the same time.
    pub fn is_concurrent(&self) -> bool { !matches!(self, SpaceQualifier::Seq) }

    /// The name of the qualifier in a constructor and in a config record.
    pub const fn as_str(self) -> &'static str {
        match self {
            SpaceQualifier::Default => "default",
            SpaceQualifier::Temp => "temp",
            SpaceQualifier::Seq => "seq",
        }
    }

    /// The qualifier with the name `name`.
    pub fn parse(name: &str) -> Option<SpaceQualifier> {
        match name {
            "default" => Some(SpaceQualifier::Default),
            "temp" => Some(SpaceQualifier::Temp),
            "seq" => Some(SpaceQualifier::Seq),
            _ => None,
        }
    }
}

impl fmt::Display for SpaceQualifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(self.as_str()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [SpaceQualifier; 3] = [
        SpaceQualifier::Default,
        SpaceQualifier::Temp,
        SpaceQualifier::Seq,
    ];

    #[test]
    fn space_qualifier_default_is_persistent_mobile_and_concurrent() {
        let qualifier = SpaceQualifier::default();
        assert_eq!(qualifier, SpaceQualifier::Default);
        assert!(qualifier.is_mobile());
        assert!(qualifier.is_persistent());
        assert!(qualifier.is_concurrent());
    }

    #[test]
    fn space_qualifier_temp_is_mobile_and_concurrent_but_not_persistent() {
        let qualifier = SpaceQualifier::Temp;
        assert!(qualifier.is_mobile());
        assert!(!qualifier.is_persistent());
        assert!(qualifier.is_concurrent());
    }

    #[test]
    fn space_qualifier_seq_is_neither_mobile_nor_persistent_nor_concurrent() {
        let qualifier = SpaceQualifier::Seq;
        assert!(!qualifier.is_mobile());
        assert!(!qualifier.is_persistent());
        assert!(!qualifier.is_concurrent());
    }

    #[test]
    fn space_qualifier_names_roundtrip_and_reject_other_names() {
        for qualifier in ALL {
            assert_eq!(SpaceQualifier::parse(qualifier.as_str()), Some(qualifier));
            assert_eq!(format!("{qualifier}"), qualifier.as_str());
        }
        assert_eq!(ALL.map(SpaceQualifier::as_str), ["default", "temp", "seq"]);
        for name in [
            "",
            "Default",
            "DEFAULT",
            "temporary",
            "sequential",
            " seq",
            "seq ",
        ] {
            assert_eq!(SpaceQualifier::parse(name), None, "{name:?}");
        }
    }
}
