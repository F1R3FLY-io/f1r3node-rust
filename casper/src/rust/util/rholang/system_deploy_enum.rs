// Enum wrapper for heterogeneous system deploys

use crate::rust::util::rholang::costacc::close_block_deploy::CloseBlockDeploy;
use crate::rust::util::rholang::costacc::redeem_deploy::RedeemDeploy;
use crate::rust::util::rholang::costacc::slash_deploy::SlashDeploy;

/// Enum to hold different types of system deploys in a homogeneous collection
#[derive(Clone)]
pub enum SystemDeployEnum {
    Slash(SlashDeploy),
    Close(CloseBlockDeploy),
    /// Cost-Accounted Rho Stage-C validator redemption (DR-7/DR-12). Unlike
    /// `Slash`/`Close`, a `Redeem` is GOVERNANCE-triggered (not auto-emitted by
    /// the block creator); it enters a block body when a redemption is proposed.
    Redeem(RedeemDeploy),
}

impl SystemDeployEnum {
    pub fn as_slash(&self) -> Option<&SlashDeploy> {
        match self {
            SystemDeployEnum::Slash(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_close(&self) -> Option<&CloseBlockDeploy> {
        match self {
            SystemDeployEnum::Close(c) => Some(c),
            _ => None,
        }
    }

    pub fn as_redeem(&self) -> Option<&RedeemDeploy> {
        match self {
            SystemDeployEnum::Redeem(r) => Some(r),
            _ => None,
        }
    }

    pub fn as_slash_mut(&mut self) -> Option<&mut SlashDeploy> {
        match self {
            SystemDeployEnum::Slash(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_close_mut(&mut self) -> Option<&mut CloseBlockDeploy> {
        match self {
            SystemDeployEnum::Close(c) => Some(c),
            _ => None,
        }
    }

    pub fn as_redeem_mut(&mut self) -> Option<&mut RedeemDeploy> {
        match self {
            SystemDeployEnum::Redeem(r) => Some(r),
            _ => None,
        }
    }
}
