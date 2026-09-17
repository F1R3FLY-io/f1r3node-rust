use super::*;

#[derive(Clone)]
pub struct ApplyPhloCostDeploy {
    amounts: ApplyCostDeploy,
    resource_cursor: Option<(MonetaryCursorTransition, i64)>,
}

impl ApplyPhloCostDeploy {
    pub fn new(
        amounts: ApplyCostDeploy,
        resource: Option<MonetaryCursorTransition>,
        fee: Option<MonetaryCursorTransition>,
        payer_count: NonZeroUsize,
    ) -> Result<Self, CasperError> {
        if amounts.fee_cursor.is_some() {
            return Err(CasperError::InvalidCostSettlement(
                "phlo settlement requires an unbound amount request".to_string(),
            ));
        }
        let reservation_id = amounts.reservation_id.as_slice().try_into().map_err(|_| {
            CasperError::InvalidCostSettlement("invalid cost reservation identity".to_string())
        })?;
        let mut amounts = ApplyCostDeploy::new(
            reservation_id,
            amounts.allocations,
            amounts.settlements,
            amounts.fee_address,
            amounts.initial_rand,
        )?;
        let has_resource = amounts.settlements.iter().any(|row| row.burn > 0);
        let has_fee = amounts.settlements.iter().any(|row| row.fee > 0);
        if resource.is_some() != has_resource || fee.is_some() != has_fee {
            return Err(CasperError::InvalidCostSettlement(
                "cursor presence must match positive realized contributions".to_string(),
            ));
        }
        if let (Some(resource), Some(fee)) = (&resource, &fee) {
            if resource.scope() == fee.scope() {
                return Err(CasperError::InvalidCostSettlement(
                    "resource and fee cursor scopes must differ".to_string(),
                ));
            }
        }
        let resource_cursor = resource
            .map(|transition| -> Result<_, CasperError> {
                transition
                    .checked_successor(transition.scope(), transition.expected(), payer_count)
                    .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
                let count = i64::try_from(payer_count.get()).map_err(|_| {
                    CasperError::InvalidCostSettlement(
                        "monetary payer count exceeds i64".to_string(),
                    )
                })?;
                Ok((transition, count))
            })
            .transpose()?;
        if let Some(fee) = fee {
            amounts = amounts.with_fee_cursor(fee, payer_count)?;
        }
        Ok(Self {
            amounts,
            resource_cursor,
        })
    }
}

impl SystemDeployTrait for ApplyPhloCostDeploy {
    type Output = (RhoBoolean, Either<RhoString, RhoNil>);
    type Result = ();

    fn source() -> &'static str {
        r#"
        new rl(`rho:registry:lookup`), systemVaultCh,
            reservationId(`sys:casper:costReservationId`),
            allocations(`sys:casper:costAllocations`),
            charges(`sys:casper:costSettlements`),
            feeAddress(`sys:casper:costFeeAddress`),
            resourceCursor(`sys:casper:costResourceCursor`),
            feeCursor(`sys:casper:costFeeCursor`),
            sysAuthToken(`sys:casper:authToken`),
            return(`sys:casper:return`) in {
          rl!(`rho:vault:system`, *systemVaultCh) |
          for (@(_, systemVault) <- systemVaultCh) {
            match (*resourceCursor, *feeCursor) {
              ([resource], [fee]) => {
                @systemVault!("applyPhloCost", *reservationId, *allocations, *charges,
                              *feeAddress, resource, fee, *sysAuthToken, *return)
              }
              _ => { return!((false, "Invalid funding cursor transport")) }
            }
          }
        }
        "#
    }

    fn process_result(
        value: <Self::Output as Extractor>::RustType,
    ) -> Either<SystemDeployUserError, ()> {
        process_result(value)
    }

    fn as_any(&self) -> &dyn std::any::Any { self }

    fn rand(&self) -> Blake2b512Random { self.amounts.rand() }

    fn env(&mut self) -> HashMap<String, Par> {
        let mut env = self.amounts.env();
        let resource = match &self.resource_cursor {
            None => RhoNil::create_par(),
            Some((transition, count)) => Par {
                exprs: vec![Expr {
                    expr_instance: Some(ExprInstance::ETupleBody(ETuple {
                        ps: vec![
                            RhoByteArray::create_par(transition.scope().to_vec()),
                            RhoNumber::create_par(*count),
                            RhoNumber::create_par(transition.expected().revision()),
                            RhoNumber::create_par(transition.expected().position()),
                            RhoNumber::create_par(transition.next().revision()),
                            RhoNumber::create_par(transition.next().position()),
                        ],
                        ..Default::default()
                    })),
                }],
                ..Default::default()
            },
        };
        env.insert(
            "sys:casper:costResourceCursor".to_string(),
            RhoList::create_par(vec![resource]),
        );
        env
    }

    fn return_channel(&mut self) -> Result<Par, CasperError> { return_channel(self.env()) }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use rholang::rust::interpreter::accounting::monetary_allocation::MonetaryCursor;
    use rholang::rust::interpreter::util::vault_address::VaultAddress;

    use super::*;

    fn amounts(burn: i64, fee: i64) -> ApplyCostDeploy {
        let address = VaultAddress::from_unforgeable(&models::rhoapi::GPrivate { id: vec![7; 32] })
            .to_base58();
        ApplyCostDeploy::new(
            [7; 32],
            vec![VaultAllocation::new(address.clone(), burn + fee + 1).unwrap()],
            vec![VaultSettlement::new(address.clone(), burn, fee).unwrap()],
            address,
            Blake2b512Random::create_from_bytes(&[7]),
        )
        .unwrap()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]
        #[test]
        fn cursor_transport_and_presence_match_contribution_invariants(
            payers in 1usize..1025, revision in 0i64..i64::MAX,
            position in any::<u16>(), next in any::<u16>(),
            burn in 0i64..1000, fee in 0i64..1000,
            resource_present in any::<bool>(), fee_present in any::<bool>(),
        ) {
            let count = NonZeroUsize::new(payers).unwrap();
            let expected = MonetaryCursor::new(revision, i64::from(position) % payers as i64, count).unwrap();
            let next_position = i64::from(next) % payers as i64;
            let resource = resource_present.then(|| MonetaryCursorTransition::new([1; 32], expected, next_position, count).unwrap());
            let fee_cursor = fee_present.then(|| MonetaryCursorTransition::new([2; 32], expected, next_position, count).unwrap());
            let result = ApplyPhloCostDeploy::new(amounts(burn, fee), resource, fee_cursor, count);
            prop_assert_eq!(result.is_ok(), resource_present == (burn > 0) && fee_present == (fee > 0));
            if let Ok(mut deploy) = result {
                let env = deploy.env();
                for (key, present, scope) in [
                    ("sys:casper:costResourceCursor", resource_present, [1; 32]),
                    ("sys:casper:costFeeCursor", fee_present, [2; 32]),
                ] {
                    let wrapped = RhoList::unapply(&env[key]).unwrap();
                    prop_assert_eq!(wrapped.len(), 1);
                    if present {
                        let Some(ExprInstance::ETupleBody(tuple)) = &wrapped[0].exprs[0].expr_instance else { panic!("cursor tuple required") };
                        prop_assert_eq!(RhoByteArray::unapply(&tuple.ps[0]).unwrap(), scope.to_vec());
                        let values: Vec<_> = tuple.ps[1..].iter().map(|par| RhoNumber::unapply(par).unwrap()).collect();
                        prop_assert_eq!(values, vec![payers as i64, revision, expected.position(), revision + 1, next_position]);
                    } else {
                        prop_assert_eq!(&wrapped[0], &RhoNil::create_par());
                    }
                }
            }
        }
    }

    #[test]
    fn rejects_aliased_roles_prebound_requests_and_mutated_amounts() {
        let count = NonZeroUsize::new(2).unwrap();
        let transition =
            MonetaryCursorTransition::new([1; 32], MonetaryCursor::INITIAL, 1, count).unwrap();
        assert!(ApplyPhloCostDeploy::new(
            amounts(1, 1),
            Some(transition.clone()),
            Some(transition.clone()),
            count
        )
        .is_err());
        assert!(ApplyPhloCostDeploy::new(
            amounts(0, 1)
                .with_fee_cursor(transition.clone(), count)
                .unwrap(),
            None,
            Some(transition),
            count
        )
        .is_err());
        let mut malformed = amounts(0, 0);
        malformed.allocations[0].amount = -1;
        assert!(ApplyPhloCostDeploy::new(malformed, None, None, count).is_err());
        let mut malformed = amounts(0, 0);
        malformed.reservation_id.clear();
        assert!(ApplyPhloCostDeploy::new(malformed, None, None, count).is_err());
    }
}
