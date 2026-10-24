#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
proof_dir="$(mktemp -d "${TMPDIR:-/tmp}/sorter-proofs.XXXXXXXX")"
trap 'rm -rf -- "$proof_dir"' EXIT
mkdir -p "$proof_dir/StackSafePDA" "$proof_dir/CostAccountedRho"
cp "$repo_root/formal/rocq/stack_safe_pda/theories/"*.v "$proof_dir/StackSafePDA/"
cp "$repo_root/formal/rocq/cost_accounted_rho/theories/StackSafeSignatureSort.v" \
  "$proof_dir/CostAccountedRho/"
rocq_paths=(-Q "$proof_dir/StackSafePDA" StackSafePDA \
            -Q "$proof_dir/CostAccountedRho" CostAccountedRho)

if rg -n '\b(Axiom|Admitted|admit)\b' "$proof_dir/StackSafePDA"/*.v; then
  exit 1
fi

coqc "${rocq_paths[@]}" "$proof_dir/CostAccountedRho/StackSafeSignatureSort.v"
for module in StackSafePDA SorterSimulation ScoreComparison CostAccountingSorter; do
  coqc "${rocq_paths[@]}" "$proof_dir/StackSafePDA/$module.v"
done

cat > "$proof_dir/Assumptions.v" <<'EOF'
From StackSafePDA Require Import StackSafePDA SorterSimulation ScoreComparison CostAccountingSorter.
Print Assumptions compile_run_equivalence.
Print Assumptions pda_fold_equivalent_to_recursive_fold.
Print Assumptions compiled_program_has_one_instruction_per_node.
Print Assumptions lazy_step_matches_reference.
Print Assumptions reference_step_matches_lazy.
Print Assumptions related_final_values.
Print Assumptions every_lazy_step_decreases_weight.
Print Assumptions sorter_initial_result.
Print Assumptions sorter_terminates.
Print Assumptions sorter_returns_recursive_result.
Print Assumptions advance_preserves_comparison.
Print Assumptions advance_decreases_weight.
Print Assumptions iterative_score_comparison_equals_recursive.
Print Assumptions signed_body_signature_and_price_are_preserved.
Print Assumptions stack_cells_keep_source_order.
Print Assumptions receive_signature_follows_patterns_and_source.
Print Assumptions par_cost_families_follow_conditionals.
Print Assumptions signature_flatten_keeps_every_leaf_and_tie_order.
EOF
coqc "${rocq_paths[@]}" "$proof_dir/Assumptions.v" > "$proof_dir/assumptions.log"
cat "$proof_dir/assumptions.log"
test "$(rg -c '^Closed under the global context$' "$proof_dir/assumptions.log")" -eq 18
coqchk "${rocq_paths[@]}" StackSafePDA.SorterSimulation StackSafePDA.ScoreComparison \
  StackSafePDA.CostAccountingSorter > "$proof_dir/kernel.log" 2>&1
if ! rg -q 'Modules were successfully checked' "$proof_dir/kernel.log"; then
  cat "$proof_dir/kernel.log"
  exit 1
fi
printf '%s\n' 'Sorter proofs passed: 18 closed theorem reports, four negative controls, and kernel verification.'
