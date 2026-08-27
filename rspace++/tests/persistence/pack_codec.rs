use crate::rspace::persistence::{
    LogPackFormatError, LogPackSequence, PACK_HEADER_LEN_V1, PackFormatError, PackHeaderV1, PackId,
    PackTerminalIdentityV1, PhysicalDigest, StoreId,
};

fn rewrite_u64(bytes: &mut [u8; PACK_HEADER_LEN_V1 as usize], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    let checksum = crate::rspace::persistence::checksum::crc32c(&bytes[..120]);
    bytes[120..].copy_from_slice(&crate::rspace::persistence::checksum::encode_crc32c(checksum));
}

fn successor() -> (PackHeaderV1, PackTerminalIdentityV1) {
    let terminal = PackTerminalIdentityV1::new(
        PackId::new(7).unwrap(),
        LogPackSequence::new(99).unwrap(),
        PhysicalDigest::of_bytes(b"pack seven terminal commit"),
    );
    (PackHeaderV1::successor(StoreId::from_array([0x22; 16]), terminal).unwrap(), terminal)
}

#[test]
fn genesis_header_round_trips_and_has_a_frozen_portable_image() {
    let header = PackHeaderV1::genesis(StoreId::from_array([0x11; 16]));
    let bytes = header.encode();
    assert_eq!(PackHeaderV1::parse_exact(&bytes).unwrap(), header);
    assert_eq!(bytes.len(), 128);
    assert_eq!(
        hex::encode(bytes),
        "5253706163655061636b2e56310000000100000000000000800000000000000011111111111111111111111111111111010000000000000001000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000ef90711f00000000"
    );
}

#[test]
fn every_pack_header_truncation_and_checksum_width_mutant_fails_closed() {
    let bytes = PackHeaderV1::genesis(StoreId::from_array([0x33; 16])).encode();
    for length in 0..bytes.len() {
        assert!(PackHeaderV1::parse_exact(&bytes[..length]).is_err(), "truncation at {length}");
    }

    let mut corrupt = bytes;
    corrupt[40] ^= 1;
    assert!(matches!(PackHeaderV1::parse_exact(&corrupt), Err(PackFormatError::Checksum { .. })));

    let mut wide = bytes;
    wide[127] = 1;
    assert!(matches!(
        PackHeaderV1::parse_exact(&wide),
        Err(PackFormatError::ChecksumOutOfRange(_))
    ));
}

#[test]
fn valid_checksum_field_mutants_cannot_reinterpret_pack_identity() {
    let bytes = PackHeaderV1::genesis(StoreId::from_array([0x44; 16])).encode();

    let mut domain = bytes;
    domain[0] ^= 1;
    let checksum = crate::rspace::persistence::checksum::crc32c(&domain[..120]);
    domain[120..].copy_from_slice(&crate::rspace::persistence::checksum::encode_crc32c(checksum));
    assert_eq!(PackHeaderV1::parse_exact(&domain), Err(PackFormatError::InvalidDomain));

    let mut version = bytes;
    rewrite_u64(&mut version, 16, 2);
    assert_eq!(PackHeaderV1::parse_exact(&version), Err(PackFormatError::UnsupportedVersion(2)));

    let mut length = bytes;
    rewrite_u64(&mut length, 24, 127);
    assert_eq!(PackHeaderV1::parse_exact(&length), Err(PackFormatError::DeclaredLength(127)));

    let mut pack_zero = bytes;
    rewrite_u64(&mut pack_zero, 48, 0);
    assert!(matches!(
        PackHeaderV1::parse_exact(&pack_zero),
        Err(PackFormatError::LogPack(LogPackFormatError::ZeroPackId))
    ));

    let mut sequence_zero = bytes;
    rewrite_u64(&mut sequence_zero, 56, 0);
    assert!(matches!(
        PackHeaderV1::parse_exact(&sequence_zero),
        Err(PackFormatError::LogPack(LogPackFormatError::ZeroSequence))
    ));

    let mut flags = bytes;
    rewrite_u64(&mut flags, 104, 1);
    assert_eq!(PackHeaderV1::parse_exact(&flags), Err(PackFormatError::UnsupportedFlags(1)));

    let mut reserved = bytes;
    rewrite_u64(&mut reserved, 112, 1);
    assert_eq!(PackHeaderV1::parse_exact(&reserved), Err(PackFormatError::NonzeroReserved(1)));

    let mut false_genesis = bytes;
    false_genesis[72] = 1;
    let checksum = crate::rspace::persistence::checksum::crc32c(&false_genesis[..120]);
    false_genesis[120..]
        .copy_from_slice(&crate::rspace::persistence::checksum::encode_crc32c(checksum));
    assert!(matches!(
        PackHeaderV1::parse_exact(&false_genesis),
        Err(PackFormatError::InvalidGenesis { .. })
    ));
}

#[test]
fn successor_header_binds_consecutive_pack_sequence_and_terminal_digest() {
    let (header, terminal) = successor();
    let parsed = PackHeaderV1::parse_exact(&header.encode()).unwrap();
    assert_eq!(parsed.pack_id(), PackId::new(8).unwrap());
    assert_eq!(parsed.first_sequence(), LogPackSequence::new(100).unwrap());
    assert_eq!(parsed.previous_pack_id(), Some(terminal.pack_id()));
    assert_eq!(parsed.previous_terminal_commit(), terminal.commit_digest());
    parsed.verify_predecessor(terminal).unwrap();
    parsed
        .verify_identity(StoreId::from_array([0x22; 16]), PackId::new(8).unwrap())
        .unwrap();

    let wrong_terminal = PackTerminalIdentityV1::new(
        terminal.pack_id(),
        terminal.last_sequence(),
        PhysicalDigest::of_bytes(b"wrong terminal"),
    );
    assert!(matches!(
        parsed.verify_predecessor(wrong_terminal),
        Err(PackFormatError::PredecessorDigest { .. })
    ));
    assert!(matches!(
        parsed.verify_identity(StoreId::from_array([0x23; 16]), parsed.pack_id()),
        Err(PackFormatError::Store { .. })
    ));
}

#[test]
fn successor_field_mutants_and_counter_overflow_fail_closed() {
    let (header, _) = successor();
    let bytes = header.encode();

    let mut nonconsecutive = bytes;
    rewrite_u64(&mut nonconsecutive, 48, 9);
    assert!(matches!(
        PackHeaderV1::parse_exact(&nonconsecutive),
        Err(PackFormatError::NonconsecutivePack { .. })
    ));

    let mut sequence_one = bytes;
    rewrite_u64(&mut sequence_one, 56, 1);
    assert_eq!(
        PackHeaderV1::parse_exact(&sequence_one),
        Err(PackFormatError::SuccessorStartsAtGenesisSequence)
    );

    let terminal_pack_exhausted = PackTerminalIdentityV1::new(
        PackId::new(u64::MAX).unwrap(),
        LogPackSequence::new(5).unwrap(),
        PhysicalDigest::ZERO,
    );
    assert!(matches!(
        PackHeaderV1::successor(StoreId::from_array([1; 16]), terminal_pack_exhausted),
        Err(PackFormatError::LogPack(LogPackFormatError::PackIdOverflow(u64::MAX)))
    ));

    let terminal_sequence_exhausted = PackTerminalIdentityV1::new(
        PackId::new(5).unwrap(),
        LogPackSequence::new(u64::MAX).unwrap(),
        PhysicalDigest::ZERO,
    );
    assert!(matches!(
        PackHeaderV1::successor(StoreId::from_array([1; 16]), terminal_sequence_exhausted),
        Err(PackFormatError::LogPack(LogPackFormatError::SequenceOverflow(u64::MAX)))
    ));
}
