// Extension trait for ProcessContext to simplify contract call creation

use rholang::rust::interpreter::contract_call::ContractCall;
use rholang::rust::interpreter::system_processes::ProcessContext;

pub trait ProcessContextExt {
    fn contract_call(&self) -> ContractCall;
}

impl ProcessContextExt for ProcessContext {
    fn contract_call(&self) -> ContractCall {
        ContractCall {
            space: self.space.clone(),
            dispatcher: self.dispatcher.clone(),
        }
    }
}
