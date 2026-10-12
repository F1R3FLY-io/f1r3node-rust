// See casper/src/test/scala/coop/rchain/casper/helper/BondingUtil.scala

use casper::rust::errors::CasperError;
use casper::rust::util::construct_deploy;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::signed::Signed;
use models::rust::casper::protocol::casper_message::DeployData;

/// Creates a bonding deploy
/// Scala equivalent: BondingUtil.bondingDeploy[F]
///
/// The Scala original accepts `amount` and then bonds a hardcoded 1000; this
/// honors it, so a spec can choose a stake geometry. Every existing caller
/// passes 1000, so their behavior is unchanged.
pub fn bonding_deploy(
    amount: i64,
    private_key: &PrivateKey,
    shard_id: Option<String>,
) -> Result<Signed<DeployData>, CasperError> {
    let source = format!(
        r#"
new retCh, PoSCh, rl(`rho:registry:lookup`), stdout(`rho:io:stdout`), deployerId(`rho:system:deployerId`) in {{
  rl!(`rho:system:pos`, *PoSCh) |
  for(@(_, PoS) <- PoSCh) {{
    @PoS!("bond", *deployerId, {amount}, *retCh)
  }}
}}
"#
    );

    construct_deploy::source_deploy_now_full(
        source,
        None,
        None,
        Some(private_key.clone()),
        None,
        shard_id,
    )
}
