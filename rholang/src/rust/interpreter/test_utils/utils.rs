use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::process::Command;

use models::rhoapi::Par;
use rholang_parser::SourcePos;

use crate::rust::interpreter::compiler::exports::{
    BoundMapChain, FreeMap, IdContextPos, NameVisitInputs, ProcVisitInputs,
};
use crate::rust::interpreter::compiler::normalize::VarSort;
use crate::rust::interpreter::compiler::normalize::VarSort::{NameSort, ProcSort};

// Helper for skipping PeTTa tests if runtime pre-requisites are not met (or
// panicking if tests are mandatory).
pub fn should_skip_petta_test() -> bool {
    let require = env::var_os("REQUIRE_PETTA_TESTS").is_some();

    let petta_path = PathBuf::from(env::var("PETTA_PATH").unwrap_or("./PeTTa".into()));
    let metta_module_path = petta_path.join("src/metta.pl");

    let petta_missing = !metta_module_path.exists();
    let swipl_missing = Command::new("swipl")
        .arg("--version")
        .output()
        .map(|output| !output.status.success())
        .unwrap_or(true);

    let error_message = match (petta_missing, swipl_missing) {
        (false, false) => return false,
        (true, _) => "PeTTa test prerequisite unmet: PeTTa is missing".to_string(),
        (_, true) => "PeTTa test prerequisite unmet: swipl is missing".to_string(),
    };

    if require {
        panic!("{error_message}");
    } else {
        eprintln!("Skipping test: {error_message}");
        true
    }
}

pub fn name_visit_inputs_and_env() -> (NameVisitInputs, HashMap<String, Par>) {
    let input: NameVisitInputs = NameVisitInputs {
        bound_map_chain: BoundMapChain::default(),
        free_map: FreeMap::default(),
    };
    let env: HashMap<String, Par> = HashMap::new();

    (input, env)
}

pub fn proc_visit_inputs_and_env() -> (ProcVisitInputs, HashMap<String, Par>) {
    let proc_inputs = ProcVisitInputs {
        par: Default::default(),
        bound_map_chain: BoundMapChain::new(),
        free_map: Default::default(),
    };
    let env: HashMap<String, Par> = HashMap::new();

    (proc_inputs, env)
}

pub fn collection_proc_visit_inputs_and_env() -> (ProcVisitInputs, HashMap<String, Par>) {
    let proc_inputs = ProcVisitInputs {
        par: Default::default(),
        bound_map_chain: {
            let bound_map_chain = BoundMapChain::new();
            bound_map_chain.put_all_pos(vec![
                (
                    "P".to_string(),
                    ProcSort,
                    SourcePos { line: 1, col: 1 }, // Use 1-based indexing consistent with rholang-rs
                ),
                ("x".to_string(), NameSort, SourcePos { line: 1, col: 1 }),
            ])
        },
        free_map: Default::default(),
    };
    let env: HashMap<String, Par> = HashMap::new();

    (proc_inputs, env)
}

pub fn proc_visit_inputs_with_updated_bound_map_chain(
    input: ProcVisitInputs,
    name: &str,
    vs_type: VarSort,
) -> ProcVisitInputs {
    ProcVisitInputs {
        bound_map_chain: {
            input.bound_map_chain.put_pos((
                name.to_string(),
                vs_type,
                SourcePos { line: 1, col: 1 }, // Use 1-based indexing
            ))
        },
        ..input.clone()
    }
}

pub fn proc_visit_inputs_with_updated_vec_bound_map_chain(
    input: ProcVisitInputs,
    new_bindings: Vec<(String, VarSort)>,
) -> ProcVisitInputs {
    let bindings_with_default_positions: Vec<IdContextPos<VarSort>> = new_bindings
        .into_iter()
        .map(|(name, var_sort)| (name, var_sort, SourcePos { line: 1, col: 1 }))
        .collect();

    ProcVisitInputs {
        bound_map_chain: {
            input
                .bound_map_chain
                .put_all_pos(bindings_with_default_positions)
        },
        ..input.clone()
    }
}
