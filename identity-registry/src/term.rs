// Rholang term builders for each registry operation. Templates live under
// rholang/identity_registry/ and are embedded at build time via include_str!.
// Placeholders are plain {{var}} tokens substituted via str::replace.
// Each term looks up the registry by URI, invokes the op with the client's
// args, and forwards the reply tuple to a client-chosen forgeable slot name.
// The client later queries that slot at the finalised block via
// get_data_at_name to read the reply.

use uuid::Uuid;

const REGISTER_TEMPLATE: &str = include_str!("../rholang/identity_registry/register.rho");
const ROTATE_TEMPLATE: &str = include_str!("../rholang/identity_registry/rotate.rho");
const ADMIN_RELINK_TEMPLATE: &str = include_str!("../rholang/identity_registry/admin_relink.rho");
const RESOLVE_CURRENT_TEMPLATE: &str =
    include_str!("../rholang/identity_registry/resolve_current.rho");
const RESOLVE_HISTORY_TEMPLATE: &str =
    include_str!("../rholang/identity_registry/resolve_history.rho");

/// Fresh reply slot name. The client passes this to the term builder and
/// later to fetch_reply to pull the result out.
pub fn new_reply_slot() -> String {
    format!("identity_registry_reply_{}", Uuid::new_v4().as_simple())
}

pub fn register_term(uri: &str, pk: &[u8], nonce: &[u8], sig: &[u8], reply_slot: &str) -> String {
    REGISTER_TEMPLATE
        .replace("{{uri}}", uri)
        .replace("{{pk_hex}}", &hex::encode(pk))
        .replace("{{nonce_hex}}", &hex::encode(nonce))
        .replace("{{sig_hex}}", &hex::encode(sig))
        .replace("{{reply_slot}}", reply_slot)
}

pub fn rotate_term(
    uri: &str,
    name: &[u8],
    new_pk: &[u8],
    seq: u64,
    sig: &[u8],
    new_sig: &[u8],
    reply_slot: &str,
) -> String {
    ROTATE_TEMPLATE
        .replace("{{uri}}", uri)
        .replace("{{name_hex}}", &hex::encode(name))
        .replace("{{new_pk_hex}}", &hex::encode(new_pk))
        .replace("{{seq}}", &seq.to_string())
        .replace("{{sig_hex}}", &hex::encode(sig))
        .replace("{{new_sig_hex}}", &hex::encode(new_sig))
        .replace("{{reply_slot}}", reply_slot)
}

#[allow(clippy::too_many_arguments)]
pub fn admin_relink_term(
    uri: &str,
    name: &[u8],
    new_pk: &[u8],
    seq: u64,
    admin_evidence: &[u8],
    admin_sigs: &[(&[u8], &[u8])],
    new_sig: &[u8],
    reply_slot: &str,
) -> String {
    let admin_sigs = admin_sigs
        .iter()
        .map(|(pk, sig)| {
            format!(
                "(\"{}\".hexToBytes(), \"{}\".hexToBytes())",
                hex::encode(pk),
                hex::encode(sig)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    ADMIN_RELINK_TEMPLATE
        .replace("{{uri}}", uri)
        .replace("{{name_hex}}", &hex::encode(name))
        .replace("{{new_pk_hex}}", &hex::encode(new_pk))
        .replace("{{seq}}", &seq.to_string())
        .replace("{{admin_evidence_hex}}", &hex::encode(admin_evidence))
        .replace("{{admin_sigs}}", &format!("[{admin_sigs}]"))
        .replace("{{new_sig_hex}}", &hex::encode(new_sig))
        .replace("{{reply_slot}}", reply_slot)
}

pub fn resolve_current_term(uri: &str, name: &[u8], reply_slot: &str) -> String {
    RESOLVE_CURRENT_TEMPLATE
        .replace("{{uri}}", uri)
        .replace("{{name_hex}}", &hex::encode(name))
        .replace("{{reply_slot}}", reply_slot)
}

pub fn resolve_history_term(uri: &str, name: &[u8], reply_slot: &str) -> String {
    RESOLVE_HISTORY_TEMPLATE
        .replace("{{uri}}", uri)
        .replace("{{name_hex}}", &hex::encode(name))
        .replace("{{reply_slot}}", reply_slot)
}
