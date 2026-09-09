// See casper/src/main/scala/coop/rchain/casper/genesis/contracts/VaultsGenerator.scala

use rholang::rust::interpreter::util::vault_address::VaultAddress;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct GenesisVaultAllocation {
    pub vault_address: VaultAddress,
    pub general_balance: u64,
    pub validator_fuel_balance: u64,
}

pub struct VaultsGenerator {
    pub supply: i64,
    pub code: String,
}

impl VaultsGenerator {
    pub fn new(supply: i64, code: String) -> Self { Self { supply, code } }

    pub fn create_from_allocations(
        allocations: Vec<GenesisVaultAllocation>,
        supply: i64,
        is_last_batch: bool,
    ) -> Self {
        let vault_balance_list = allocations
            .iter()
            .map(|v| {
                format!(
                    "(\"{}\", {}, {})",
                    v.vault_address.to_base58(),
                    v.general_balance,
                    v.validator_fuel_balance
                )
            })
            .collect::<Vec<String>>()
            .join(", ");

        let continue_clause = if !is_last_batch {
            "| initContinue!()"
        } else {
            ""
        };

        let code = format!(
            r#" 
            new rl(`rho:registry:lookup`), systemVaultCh in {{
              rl!(`rho:vault:system`, *systemVaultCh) |
              for (@(_, SystemVault) <- systemVaultCh) {{
                new systemVaultInitCh in {{
                  @SystemVault!("init", *systemVaultInitCh) |
                  for (TreeHashMap, @vaultMap, initVault, initContinue <- systemVaultInitCh) {{
                    match [{}] {{
                      vaults => {{
                        new iter in {{
                          contract iter(@[(addr, initialGeneralBalance, initialValidatorFuelBalance) ... tail]) = {{
                          iter!(tail) |
                          new vault, setDoneCh in {{
                            initVault!(*vault, addr, initialGeneralBalance, initialValidatorFuelBalance) |
                            TreeHashMap!("set", vaultMap, addr, *vault, *setDoneCh) |
                            for (_ <- setDoneCh) {{ Nil }}
                          }}
                      }} |
                      iter!(vaults) {}
                    }}
                  }}
                }}
              }}
            }}
          }}
        }}
      "#,
            vault_balance_list, continue_clause
        );

        Self::new(supply, code)
    }
}
