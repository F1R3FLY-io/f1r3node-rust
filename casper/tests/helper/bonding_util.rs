// See casper/src/test/scala/coop/rchain/casper/helper/BondingUtil.scala

use casper::rust::errors::CasperError;
// Changed by DR-134 (DR-66 item 1): the bonding deploy is an offered deploy.
// use casper::rust::util::construct_deploy;
// use crypto::rust::signatures::signed::Signed;
// use models::rust::casper::protocol::casper_message::DeployData;
use crypto::rust::private_key::PrivateKey;

use crate::helper::offered_deploy as construct_deploy;
use crate::helper::offered_deploy::OfferedDeploy;

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
    // Changed by DR-134 (DR-66 item 1).
    // ) -> Result<Signed<DeployData>, CasperError> {
) -> Result<OfferedDeploy, CasperError> {
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
