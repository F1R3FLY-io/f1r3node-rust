use std::collections::{BTreeMap, HashMap};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{ETuple, Expr, Par};
use rholang::rust::interpreter::rho_type::{
    Extractor, RhoBoolean, RhoByteArray, RhoList, RhoNil, RhoNumber, RhoString, RhoTuple2,
};
use rspace_plus_plus::rspace::history::Either;

use crate::rust::errors::CasperError;
use crate::rust::util::rholang::system_deploy::SystemDeployTrait;
use crate::rust::util::rholang::system_deploy_user_error::SystemDeployUserError;

pub fn lifecycle_random(reservation_id: &[u8; 32], phase: u8) -> Blake2b512Random {
    let mut seed = Vec::new();
    seed.extend_from_slice(b"f1r3node:vault-cost-lifecycle:v1");
    seed.extend_from_slice(reservation_id);
    seed.push(phase);
    Blake2b512Random::create_from_bytes(&seed)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultAllocation {
    pub address: String,
    pub amount: i64,
}

impl VaultAllocation {
    pub fn new(address: String, amount: i64) -> Result<Self, CasperError> {
        if amount <= 0 {
            return Err(CasperError::InvalidCostSettlement(
                "vault allocation must be positive".to_string(),
            ));
        }
        rholang::rust::interpreter::util::vault_address::VaultAddress::parse(&address)
            .map_err(CasperError::InvalidCostSettlement)?;
        Ok(Self { address, amount })
    }
}

fn canonical_allocations(
    allocations: Vec<VaultAllocation>,
) -> Result<Vec<VaultAllocation>, CasperError> {
    let mut canonical = BTreeMap::<String, i64>::new();
    for allocation in allocations {
        let allocation = VaultAllocation::new(allocation.address, allocation.amount)?;
        let amount = canonical.entry(allocation.address).or_default();
        *amount = amount.checked_add(allocation.amount).ok_or_else(|| {
            CasperError::InvalidCostSettlement("vault allocation overflow".to_string())
        })?;
    }
    if canonical.is_empty() {
        return Err(CasperError::InvalidCostSettlement(
            "cost reservation requires at least one vault allocation".to_string(),
        ));
    }
    Ok(canonical
        .into_iter()
        .map(|(address, amount)| VaultAllocation { address, amount })
        .collect())
}

fn allocation_par(allocations: &[VaultAllocation]) -> Par {
    RhoList::create_par(
        allocations
            .iter()
            .map(|allocation| {
                RhoTuple2::create_par((
                    RhoString::create_par(allocation.address.clone()),
                    RhoNumber::create_par(allocation.amount),
                ))
            })
            .collect(),
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultSettlement {
    pub address: String,
    pub burn: i64,
    pub fee: i64,
}

impl VaultSettlement {
    pub fn new(address: String, burn: i64, fee: i64) -> Result<Self, CasperError> {
        rholang::rust::interpreter::util::vault_address::VaultAddress::parse(&address)
            .map_err(CasperError::InvalidCostSettlement)?;
        if burn < 0 || fee < 0 {
            return Err(CasperError::InvalidCostSettlement(
                "vault settlement amounts must be non-negative".to_string(),
            ));
        }
        burn.checked_add(fee).ok_or_else(|| {
            CasperError::InvalidCostSettlement("vault settlement overflow".to_string())
        })?;
        Ok(Self { address, burn, fee })
    }
}

fn canonical_settlements(
    settlements: Vec<VaultSettlement>,
) -> Result<Vec<VaultSettlement>, CasperError> {
    let mut canonical = BTreeMap::<String, (i64, i64)>::new();
    for settlement in settlements {
        let settlement = VaultSettlement::new(settlement.address, settlement.burn, settlement.fee)?;
        let totals = canonical.entry(settlement.address).or_default();
        totals.0 = totals.0.checked_add(settlement.burn).ok_or_else(|| {
            CasperError::InvalidCostSettlement("vault burn total overflow".to_string())
        })?;
        totals.1 = totals.1.checked_add(settlement.fee).ok_or_else(|| {
            CasperError::InvalidCostSettlement("vault fee total overflow".to_string())
        })?;
        totals.0.checked_add(totals.1).ok_or_else(|| {
            CasperError::InvalidCostSettlement("vault settlement total overflow".to_string())
        })?;
    }
    if canonical.is_empty() {
        return Err(CasperError::InvalidCostSettlement(
            "cost settlement requires at least one vault allocation".to_string(),
        ));
    }
    canonical
        .into_iter()
        .map(|(address, (burn, fee))| VaultSettlement::new(address, burn, fee))
        .collect()
}

fn settlement_par(settlements: &[VaultSettlement]) -> Par {
    RhoList::create_par(
        settlements
            .iter()
            .map(|settlement| {
                Par::default().with_exprs(vec![Expr {
                    expr_instance: Some(ExprInstance::ETupleBody(ETuple {
                        ps: vec![
                            RhoString::create_par(settlement.address.clone()),
                            RhoNumber::create_par(settlement.burn),
                            RhoNumber::create_par(settlement.fee),
                        ],
                        locally_free: Vec::new(),
                        connective_used: false,
                    })),
                }])
            })
            .collect(),
    )
}

fn process_result(value: (bool, Either<String, ()>)) -> Either<SystemDeployUserError, ()> {
    match value {
        (true, _) => Either::Right(()),
        (false, Either::Left(error)) => Either::Left(SystemDeployUserError::new(error)),
        _ => Either::Left(SystemDeployUserError::new(
            "vault cost operation failed without a cause".to_string(),
        )),
    }
}

fn return_channel(env: HashMap<String, Par>) -> Result<Par, CasperError> {
    env.get("sys:casper:return").cloned().ok_or_else(|| {
        CasperError::RuntimeError("return channel is absent from system deploy env".to_string())
    })
}

#[derive(Clone)]
pub struct ApplyCostDeploy {
    pub reservation_id: Vec<u8>,
    pub allocations: Vec<VaultAllocation>,
    pub settlements: Vec<VaultSettlement>,
    pub fee_address: String,
    pub initial_rand: Blake2b512Random,
}

impl ApplyCostDeploy {
    pub fn new(
        reservation_id: [u8; 32],
        allocations: Vec<VaultAllocation>,
        settlements: Vec<VaultSettlement>,
        fee_address: String,
        initial_rand: Blake2b512Random,
    ) -> Result<Self, CasperError> {
        rholang::rust::interpreter::util::vault_address::VaultAddress::parse(&fee_address)
            .map_err(CasperError::InvalidCostSettlement)?;
        let allocations = canonical_allocations(allocations)?;
        let settlements = canonical_settlements(settlements)?;
        if allocations.len() != settlements.len()
            || allocations
                .iter()
                .zip(&settlements)
                .any(|(allocation, settlement)| {
                    allocation.address != settlement.address
                        || settlement
                            .burn
                            .checked_add(settlement.fee)
                            .is_none_or(|total| total > allocation.amount)
                })
        {
            return Err(CasperError::InvalidCostSettlement(
                "cost settlement allocations do not match reservation".to_string(),
            ));
        }
        Ok(Self {
            reservation_id: reservation_id.to_vec(),
            allocations,
            settlements,
            fee_address,
            initial_rand,
        })
    }
}

impl SystemDeployTrait for ApplyCostDeploy {
    type Output = (RhoBoolean, Either<RhoString, RhoNil>);
    type Result = ();

    fn source() -> &'static str {
        r#"
        new rl(`rho:registry:lookup`), systemVaultCh,
            reservationId(`sys:casper:costReservationId`),
            allocations(`sys:casper:costAllocations`),
            charges(`sys:casper:costSettlements`),
            feeAddress(`sys:casper:costFeeAddress`),
            sysAuthToken(`sys:casper:authToken`),
            return(`sys:casper:return`) in {
          rl!(`rho:vault:system`, *systemVaultCh) |
          for (@(_, systemVault) <- systemVaultCh) {
            @systemVault!("applyCost", *reservationId, *allocations, *charges, *feeAddress, *sysAuthToken, *return)
          }
        }
        "#
    }

    fn process_result(
        value: <Self::Output as Extractor>::RustType,
    ) -> Either<SystemDeployUserError, Self::Result> {
        process_result(value)
    }

    fn as_any(&self) -> &dyn std::any::Any { self }

    fn rand(&self) -> Blake2b512Random { self.initial_rand.clone() }

    fn env(&mut self) -> HashMap<String, Par> {
        let mut env = HashMap::new();
        env.insert(
            "sys:casper:costReservationId".to_string(),
            RhoByteArray::create_par(self.reservation_id.clone()),
        );
        env.insert(
            "sys:casper:costAllocations".to_string(),
            allocation_par(&self.allocations),
        );
        env.insert(
            "sys:casper:costSettlements".to_string(),
            settlement_par(&self.settlements),
        );
        env.insert(
            "sys:casper:costFeeAddress".to_string(),
            RhoString::create_par(self.fee_address.clone()),
        );
        let (key, value) = self.mk_sys_auth_token();
        env.insert(key, value);
        let (key, value) = self.mk_return_channel();
        env.insert(key, value);
        env
    }

    fn return_channel(&mut self) -> Result<Par, CasperError> { return_channel(self.env()) }
}

#[derive(Clone)]
pub struct ProtocolMintDeploy {
    pub target_address: String,
    pub amount: i64,
    pub initial_rand: Blake2b512Random,
}

impl ProtocolMintDeploy {
    pub fn new(
        target_address: String,
        amount: i64,
        initial_rand: Blake2b512Random,
    ) -> Result<Self, CasperError> {
        let allocation = VaultAllocation::new(target_address, amount)?;
        Ok(Self {
            target_address: allocation.address,
            amount: allocation.amount,
            initial_rand,
        })
    }
}

impl SystemDeployTrait for ProtocolMintDeploy {
    type Output = (RhoBoolean, Either<RhoString, RhoNil>);
    type Result = ();

    fn source() -> &'static str {
        r#"
        new rl(`rho:registry:lookup`), systemVaultCh,
            targetAddress(`sys:casper:mintTargetAddress`),
            amount(`sys:casper:mintAmount`),
            sysAuthToken(`sys:casper:authToken`),
            return(`sys:casper:return`) in {
          rl!(`rho:vault:system`, *systemVaultCh) |
          for (@(_, systemVault) <- systemVaultCh) {
            @systemVault!("protocolMint", *targetAddress, *amount, *sysAuthToken, *return)
          }
        }
        "#
    }

    fn process_result(
        value: <Self::Output as Extractor>::RustType,
    ) -> Either<SystemDeployUserError, Self::Result> {
        process_result(value)
    }

    fn as_any(&self) -> &dyn std::any::Any { self }

    fn rand(&self) -> Blake2b512Random { self.initial_rand.clone() }

    fn env(&mut self) -> HashMap<String, Par> {
        let mut env = HashMap::new();
        env.insert(
            "sys:casper:mintTargetAddress".to_string(),
            RhoString::create_par(self.target_address.clone()),
        );
        env.insert(
            "sys:casper:mintAmount".to_string(),
            RhoNumber::create_par(self.amount),
        );
        let (key, value) = self.mk_sys_auth_token();
        env.insert(key, value);
        let (key, value) = self.mk_return_channel();
        env.insert(key, value);
        env
    }

    fn return_channel(&mut self) -> Result<Par, CasperError> { return_channel(self.env()) }
}

#[derive(Clone)]
pub struct ProtocolBurnDeploy {
    pub target_address: String,
    pub amount: i64,
    pub initial_rand: Blake2b512Random,
}

impl ProtocolBurnDeploy {
    pub fn new(
        target_address: String,
        amount: i64,
        initial_rand: Blake2b512Random,
    ) -> Result<Self, CasperError> {
        rholang::rust::interpreter::util::vault_address::VaultAddress::parse(&target_address)
            .map_err(CasperError::InvalidCostSettlement)?;
        if amount < 0 {
            return Err(CasperError::InvalidCostSettlement(
                "protocol burn amount must be non-negative".to_string(),
            ));
        }
        Ok(Self {
            target_address,
            amount,
            initial_rand,
        })
    }
}

impl SystemDeployTrait for ProtocolBurnDeploy {
    type Output = (RhoBoolean, Either<RhoString, RhoNil>);
    type Result = ();

    fn source() -> &'static str {
        r#"
        new rl(`rho:registry:lookup`), systemVaultCh,
            targetAddress(`sys:casper:burnTargetAddress`),
            amount(`sys:casper:burnAmount`),
            sysAuthToken(`sys:casper:authToken`),
            return(`sys:casper:return`) in {
          rl!(`rho:vault:system`, *systemVaultCh) |
          for (@(_, systemVault) <- systemVaultCh) {
            @systemVault!("protocolBurn", *targetAddress, *amount, *sysAuthToken, *return)
          }
        }
        "#
    }

    fn process_result(
        value: <Self::Output as Extractor>::RustType,
    ) -> Either<SystemDeployUserError, Self::Result> {
        process_result(value)
    }

    fn as_any(&self) -> &dyn std::any::Any { self }

    fn rand(&self) -> Blake2b512Random { self.initial_rand.clone() }

    fn env(&mut self) -> HashMap<String, Par> {
        let mut env = HashMap::new();
        env.insert(
            "sys:casper:burnTargetAddress".to_string(),
            RhoString::create_par(self.target_address.clone()),
        );
        env.insert(
            "sys:casper:burnAmount".to_string(),
            RhoNumber::create_par(self.amount),
        );
        let (key, value) = self.mk_sys_auth_token();
        env.insert(key, value);
        let (key, value) = self.mk_return_channel();
        env.insert(key, value);
        env
    }

    fn return_channel(&mut self) -> Result<Par, CasperError> { return_channel(self.env()) }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use rholang::rust::interpreter::compiler::compiler::Compiler;

    use super::*;

    fn address(tag: u8) -> String {
        rholang::rust::interpreter::util::vault_address::VaultAddress::from_unforgeable(
            &models::rhoapi::GPrivate { id: vec![tag; 32] },
        )
        .to_base58()
    }

    #[test]
    fn cost_system_sources_compile() {
        Compiler::source_to_adt(ApplyCostDeploy::source()).unwrap();
        Compiler::source_to_adt(ProtocolMintDeploy::source()).unwrap();
        Compiler::source_to_adt(ProtocolBurnDeploy::source()).unwrap();
    }

    #[test]
    fn allocations_are_sorted_and_coalesced() {
        let first = address(1);
        let second = address(2);
        let deploy = ApplyCostDeploy::new(
            [9; 32],
            vec![
                VaultAllocation::new(second.clone(), 3).unwrap(),
                VaultAllocation::new(first.clone(), 2).unwrap(),
                VaultAllocation::new(second.clone(), 4).unwrap(),
            ],
            vec![
                VaultSettlement::new(first.clone(), 1, 1).unwrap(),
                VaultSettlement::new(second.clone(), 2, 3).unwrap(),
            ],
            first.clone(),
            Blake2b512Random::create_from_bytes(&[1]),
        )
        .unwrap();
        assert_eq!(deploy.allocations, vec![
            VaultAllocation::new(first, 2).unwrap(),
            VaultAllocation::new(second, 7).unwrap(),
        ]);
    }

    #[test]
    fn allocation_coalescing_rejects_overflow() {
        let payer = address(1);
        assert!(ApplyCostDeploy::new(
            [9; 32],
            vec![
                VaultAllocation::new(payer.clone(), i64::MAX).unwrap(),
                VaultAllocation::new(payer, 1).unwrap(),
            ],
            vec![VaultSettlement::new(address(1), 1, 0).unwrap()],
            address(2),
            Blake2b512Random::create_from_bytes(&[1]),
        )
        .is_err());
    }

    #[test]
    fn empty_and_non_positive_reservations_are_rejected() {
        assert!(ApplyCostDeploy::new(
            [0; 32],
            Vec::new(),
            vec![VaultSettlement::new(address(1), 0, 0).unwrap()],
            address(2),
            Blake2b512Random::create_from_bytes(&[1]),
        )
        .is_err());
        assert!(VaultAllocation::new(address(1), 0).is_err());
    }

    #[test]
    fn settlements_are_sorted_coalesced_and_checked() {
        let first = address(1);
        let second = address(2);
        let deploy = ApplyCostDeploy::new(
            [3; 32],
            vec![
                VaultAllocation::new(first.clone(), 12).unwrap(),
                VaultAllocation::new(second.clone(), 29).unwrap(),
            ],
            vec![
                VaultSettlement::new(second.clone(), 2, 3).unwrap(),
                VaultSettlement::new(first.clone(), 5, 7).unwrap(),
                VaultSettlement::new(second.clone(), 11, 13).unwrap(),
            ],
            first.clone(),
            Blake2b512Random::create_from_bytes(&[2]),
        )
        .unwrap();
        assert_eq!(deploy.settlements, vec![
            VaultSettlement::new(first, 5, 7).unwrap(),
            VaultSettlement::new(second, 13, 16).unwrap(),
        ]);
        assert!(VaultSettlement::new(address(3), -1, 0).is_err());
        assert!(ApplyCostDeploy::new(
            [4; 32],
            vec![VaultAllocation::new(address(4), 1).unwrap()],
            Vec::new(),
            address(4),
            Blake2b512Random::create_from_bytes(&[3]),
        )
        .is_err());
    }

    #[test]
    fn settlement_coalescing_rejects_overflow() {
        let payer = address(1);
        assert!(VaultSettlement::new(payer.clone(), i64::MAX, 1).is_err());
        assert!(ApplyCostDeploy::new(
            [3; 32],
            vec![VaultAllocation::new(payer.clone(), i64::MAX).unwrap()],
            vec![
                VaultSettlement::new(payer.clone(), i64::MAX, 0).unwrap(),
                VaultSettlement::new(payer.clone(), 1, 0).unwrap(),
            ],
            payer,
            Blake2b512Random::create_from_bytes(&[2]),
        )
        .is_err());
    }

    proptest! {
        #[test]
        fn atomic_cost_request_is_permutation_invariant(
            first_max in 1i64..10_000,
            second_max in 1i64..10_000,
            first_burn_seed in 0i64..10_000,
            first_fee_seed in 0i64..10_000,
            second_burn_seed in 0i64..10_000,
            second_fee_seed in 0i64..10_000,
        ) {
            let first = address(1);
            let second = address(2);
            let first_burn = first_burn_seed % (first_max + 1);
            let first_fee = first_fee_seed % (first_max - first_burn + 1);
            let second_burn = second_burn_seed % (second_max + 1);
            let second_fee = second_fee_seed % (second_max - second_burn + 1);
            let left = ApplyCostDeploy::new(
                [7; 32],
                vec![
                    VaultAllocation::new(first.clone(), first_max).unwrap(),
                    VaultAllocation::new(second.clone(), second_max).unwrap(),
                ],
                vec![
                    VaultSettlement::new(first.clone(), first_burn, first_fee).unwrap(),
                    VaultSettlement::new(second.clone(), second_burn, second_fee).unwrap(),
                ],
                first.clone(),
                Blake2b512Random::create_from_bytes(&[7]),
            )
            .unwrap();
            let right = ApplyCostDeploy::new(
                [7; 32],
                vec![
                    VaultAllocation::new(second.clone(), second_max).unwrap(),
                    VaultAllocation::new(first, first_max).unwrap(),
                ],
                vec![
                    VaultSettlement::new(second, second_burn, second_fee).unwrap(),
                    VaultSettlement::new(address(1), first_burn, first_fee).unwrap(),
                ],
                address(1),
                Blake2b512Random::create_from_bytes(&[7]),
            )
            .unwrap();
            prop_assert_eq!(left.allocations, right.allocations);
            prop_assert_eq!(left.settlements, right.settlements);
        }

        #[test]
        fn atomic_cost_request_rejects_realized_overdraw(
            maximum in 1i64..100_000,
            excess in 1i64..100_000,
        ) {
            let payer = address(1);
            let request = ApplyCostDeploy::new(
                [8; 32],
                vec![VaultAllocation::new(payer.clone(), maximum).unwrap()],
                vec![VaultSettlement::new(payer, maximum + excess, 0).unwrap()],
                address(2),
                Blake2b512Random::create_from_bytes(&[8]),
            );
            prop_assert!(request.is_err());
        }
    }

    #[test]
    fn protocol_burn_validates_address_and_amount() {
        assert!(
            ProtocolBurnDeploy::new(address(1), 0, Blake2b512Random::create_from_bytes(&[4]),)
                .is_ok()
        );
        assert!(
            ProtocolBurnDeploy::new(address(1), -1, Blake2b512Random::create_from_bytes(&[5]),)
                .is_err()
        );
        assert!(ProtocolBurnDeploy::new(
            "not-a-vault-address".to_string(),
            1,
            Blake2b512Random::create_from_bytes(&[6]),
        )
        .is_err());
    }
}
