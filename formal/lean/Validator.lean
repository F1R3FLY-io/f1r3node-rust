import Validator.SlashAuthorization

namespace Validator

theorem validator_contract_slash_effect := bm_slash_lookup

theorem validator_contract_slash_order := bm_slash_many_order_independent

theorem validator_contract_stale_evidence := stale_evidence_not_authorized

end Validator
