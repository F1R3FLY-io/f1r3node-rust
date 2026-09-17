use super::*;

#[test]
fn offered_wallet_authorization_preserves_native_projection_and_threshold_exclusion() {
    for count in [1, 2, 3, 17, 64] {
        let requested: Vec<_> = (0..count).map(custody).collect();
        let original = envelope(&vec![true; count], &requested);
        let offered = offered_envelope(&vec![true; count], &requested, 10, 1);
        let original = authorize_direct_wallet_funding(&original, authorization_limits()).unwrap();
        let checked =
            authorize_offered_direct_wallet_funding(&offered, authorization_limits()).unwrap();
        assert!(std::ptr::eq(checked.envelope(), &offered));
        assert_eq!(checked.record(), original.record());
        assert_eq!(checked.payers(), original.payers());
    }
    for index in [1, 3] {
        let offered = offered_envelope(&[true, false, true], &[custody(index)], 10, 1);
        assert!(matches!(
            authorize_offered_direct_wallet_funding(&offered, authorization_limits()),
            Err(DirectWalletFundingError::UnwitnessedCustody { index: 0 })
        ));
    }
}

#[test]
fn offered_wallet_authorization_rejects_unsigned_changes_and_malformed_custody() {
    let signed = offered_envelope(&[true], &[custody(0)], 10, 1);
    for (limit, price) in [(9, 1), (10, 0), (10, 2)] {
        let mut changed = signed.clone();
        changed.data = OfferedFundedDeploy::new(
            signed.data.body().clone(),
            signed.data.funding_intent().to_vec(),
            limit,
            price,
            limits(),
        )
        .unwrap();
        assert!(matches!(
            authorize_offered_direct_wallet_funding(&changed, authorization_limits()),
            Err(DirectWalletFundingError::Signature(_))
        ));
    }
    for bytes in [vec![1; 31], vec![1; 33]] {
        let offered = offered_envelope(&[true], &[bytes], 10, 1);
        assert!(matches!(
            authorize_offered_direct_wallet_funding(&offered, authorization_limits()),
            Err(DirectWalletFundingError::MalformedCustody { index: 0 })
        ));
    }
    let offered = offered_envelope(&[true, true], &[custody(0)], 10, 1);
    let mut bounded = authorization_limits();
    bounded.members = NonZeroUsize::new(1).unwrap();
    assert!(matches!(
        authorize_offered_direct_wallet_funding(&offered, bounded),
        Err(DirectWalletFundingError::TooManyMembers)
    ));
}

#[test]
fn offered_ethereum_signature_uses_the_same_wallet_mapping() {
    let base = data(&[custody(0)]);
    let offered = OfferedFundedDeploy::new(
        base.body().clone(),
        base.funding_intent().to_vec(),
        10,
        1,
        limits(),
    )
    .unwrap();
    let envelope =
        Cosigned::create_single_envelope(offered, Box::new(Secp256k1Eth), key(0)).unwrap();
    let checked =
        authorize_offered_direct_wallet_funding(&envelope, authorization_limits()).unwrap();
    assert_eq!(checked.payers().len(), 1);
    assert_eq!(
        checked.payers().keys().next().unwrap().as_slice(),
        custody(0)
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn offered_wallet_membership_matches_selected_subset(
        selected in prop::collection::vec(any::<bool>(), 1..17),
        requested in prop::collection::btree_set(0_usize..20, 0..20),
    ) {
        prop_assume!(selected.iter().any(|value| *value));
        let expected = requested.iter().all(|index| selected.get(*index) == Some(&true));
        let mut custodies: Vec<_> = requested.iter().map(|index| custody(*index)).collect();
        for _ in 0..2 {
            let signed = offered_envelope(&selected, &custodies, 10, 1);
            prop_assert_eq!(authorize_offered_direct_wallet_funding(&signed, authorization_limits()).is_ok(), expected);
            custodies.reverse();
        }
    }
}
