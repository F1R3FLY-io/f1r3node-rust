// Reads tagged replies that the registry forwarded to a client-chosen
// forgeable name. The term builders in term.rs set the slot up. Here we
// fetch what was sent and decode the ("ok", value) / ("err", reason)
// envelope.

use models::casper::v1::deploy_service_client::DeployServiceClient;
use models::casper::v1::rho_data_response::Message;
use models::casper::DataAtNameByBlockQuery;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, EMap, ETuple, Expr, Par};

use crate::error::IdentityRegistryError;

/// Builds the Par that represents the forgeable channel `@"<slot>"`.
pub fn reply_slot_par(slot: &str) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GString(slot.to_owned())),
        }],
        ..Default::default()
    }
}

/// Queries the finalised block for whatever was sent at the slot. Returns
/// the single Par that was forwarded by the contract wrapper, or an error
/// if the node is unreachable, nothing was sent, or more than one thing
/// was sent.
pub async fn fetch_reply(
    node_host: &str,
    grpc_port: u16,
    slot: &str,
    block_hash: &str,
) -> Result<Par, IdentityRegistryError> {
    let endpoint = format!("http://{node_host}:{grpc_port}/");
    let mut client = DeployServiceClient::connect(endpoint)
        .await
        .map_err(|e| IdentityRegistryError::Node(format!("connect failed: {e:?}")))?;
    let query = DataAtNameByBlockQuery {
        par: Some(reply_slot_par(slot)),
        block_hash: block_hash.to_owned(),
        use_pre_state_hash: false,
    };
    let response = client
        .get_data_at_name(query)
        .await
        .map_err(|e| IdentityRegistryError::Node(format!("get_data_at_name failed: {e:?}")))?
        .into_inner();

    match response.message {
        Some(Message::Payload(payload)) => {
            if payload.par.is_empty() {
                return Err(IdentityRegistryError::ResponseParse(
                    "no data at reply slot".into(),
                ));
            }
            if payload.par.len() > 1 {
                return Err(IdentityRegistryError::ResponseParse(format!(
                    "expected 1 message at reply slot, got {}",
                    payload.par.len()
                )));
            }
            Ok(payload.par.into_iter().next().unwrap())
        }
        Some(Message::Error(e)) => Err(IdentityRegistryError::Node(format!("{e:?}"))),
        None => Err(IdentityRegistryError::ResponseParse(
            "empty response".into(),
        )),
    }
}

/// Unwraps a `("ok", value)` or `("err", reason)` tuple. On `"err"` returns
/// `IdentityRegistryError::Rejected(reason)`. On `"ok"` returns the second
/// element, left to the caller to decode by shape.
pub fn parse_tagged_reply(reply: Par) -> Result<Par, IdentityRegistryError> {
    let tuple = extract_etuple(&reply)?;
    if tuple.ps.len() != 2 {
        return Err(IdentityRegistryError::ResponseParse(format!(
            "expected 2-tuple, got {}-tuple",
            tuple.ps.len()
        )));
    }
    let tag = extract_gstring(&tuple.ps[0])?;
    match tag.as_str() {
        "ok" => Ok(tuple.ps[1].clone()),
        "err" => {
            let reason = extract_gstring(&tuple.ps[1])?;
            Err(IdentityRegistryError::Rejected(reason))
        }
        other => Err(IdentityRegistryError::ResponseParse(format!(
            "expected tag \"ok\" or \"err\", got {other:?}"
        ))),
    }
}

pub fn extract_etuple(par: &Par) -> Result<&ETuple, IdentityRegistryError> {
    for e in &par.exprs {
        if let Some(ExprInstance::ETupleBody(t)) = &e.expr_instance {
            return Ok(t);
        }
    }
    Err(IdentityRegistryError::ResponseParse(
        "expected ETuple".into(),
    ))
}

pub fn extract_gstring(par: &Par) -> Result<String, IdentityRegistryError> {
    for e in &par.exprs {
        if let Some(ExprInstance::GString(s)) = &e.expr_instance {
            return Ok(s.clone());
        }
    }
    Err(IdentityRegistryError::ResponseParse(
        "expected GString".into(),
    ))
}

pub fn extract_gbytearray(par: &Par) -> Result<Vec<u8>, IdentityRegistryError> {
    for e in &par.exprs {
        if let Some(ExprInstance::GByteArray(b)) = &e.expr_instance {
            return Ok(b.clone());
        }
    }
    Err(IdentityRegistryError::ResponseParse(
        "expected GByteArray".into(),
    ))
}

pub fn extract_gint(par: &Par) -> Result<i64, IdentityRegistryError> {
    for e in &par.exprs {
        if let Some(ExprInstance::GInt(n)) = &e.expr_instance {
            return Ok(*n);
        }
    }
    Err(IdentityRegistryError::ResponseParse("expected GInt".into()))
}

pub fn extract_elist(par: &Par) -> Result<&EList, IdentityRegistryError> {
    for e in &par.exprs {
        if let Some(ExprInstance::EListBody(l)) = &e.expr_instance {
            return Ok(l);
        }
    }
    Err(IdentityRegistryError::ResponseParse(
        "expected EList".into(),
    ))
}

pub fn extract_emap(par: &Par) -> Result<&EMap, IdentityRegistryError> {
    for e in &par.exprs {
        if let Some(ExprInstance::EMapBody(m)) = &e.expr_instance {
            return Ok(m);
        }
    }
    Err(IdentityRegistryError::ResponseParse("expected EMap".into()))
}
