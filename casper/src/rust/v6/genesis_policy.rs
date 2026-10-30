use models::rust::block::state_hash::StateHash;
use rholang::rust::interpreter::rho_type::RhoByteArray;

use super::resource_policy::ShardResourcePolicy;
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

const RESOURCE_POLICY_QUERY: &str = r#"
    new ret, rl(`rho:registry:lookup`), tmCh in {
      rl!(`rho:system:tokenMetadata`, *tmCh) |
      for (@(_, TokenMetadata) <- tmCh) {
        @TokenMetadata!("resourcePolicy", *ret)
      }
    }
"#;

pub async fn read_genesis_policy(
    runtime_manager: &RuntimeManager,
    state_hash: &StateHash,
) -> Result<Option<ShardResourcePolicy>, CasperError> {
    let (result, _cost) = runtime_manager
        .play_exploratory_deploy(RESOURCE_POLICY_QUERY.to_string(), state_hash, None)
        .await?;
    match result.as_slice() {
        [] => Ok(None),
        [par] => {
            let bytes = RhoByteArray::unapply(par).ok_or_else(|| {
                CasperError::RuntimeError(
                    "the genesis resource policy must be a byte array".to_string(),
                )
            })?;
            ShardResourcePolicy::decode(&bytes)
                .map(Some)
                .map_err(CasperError::RuntimeError)
        }
        _ => Err(CasperError::RuntimeError(
            "genesis must hold at most one resource policy".to_string(),
        )),
    }
}
