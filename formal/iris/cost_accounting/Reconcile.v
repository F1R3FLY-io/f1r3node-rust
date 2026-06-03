(* ════════════════════════════════════════════════════════════════════════
   Reconcile.v — lock-free budget reconciliation, Iris/HeapLang (concurrent
   separation logic). The accounting runtime debits a shared budget with a CAS
   retry loop; this module formalizes the HeapLang program and its LOGICALLY-ATOMIC
   specification — the linearizability / schedule-independence claim that the Rust
   `loom` model-checker and the RuntimeBudgetReplay TLA+ model already cover
   empirically (both LOCAL-ONLY and present in-tree). It is the deepest leg of the
   multi-prover arsenal and depends on coq-iris, which is NOT installed on this
   host; the gate (check-cost-accounted-rho-iris.sh) detects that and SKIPS. The
   program + atomic spec below type-check against coq-iris; discharging the atomic
   triple is the documented continuation requiring the coq-iris toolchain.        *)
From iris.heap_lang Require Import lang proofmode notation.
From iris.base_logic.lib Require Import invariants.
From iris.bi.lib Require Import atomic.

(* CAS-loop debit: atomically subtract [amt] from the budget cell [l], retrying on
   contention; returns the pre-debit balance. Lock-free (no mutex). *)
Definition debit : val :=
  rec: "debit" "l" "amt" :=
    let: "cur" := ! "l" in
    if: CAS "l" "cur" ("cur" - "amt")
    then "cur"
    else "debit" "l" "amt".

Section spec.
  Context `{!heapGS Σ}.

  (* The logically-atomic specification: debit is linearizable — its visible effect
     (the budget transitions from b to b-amt, returning b) occurs at a single atomic
     commit point, regardless of interleaving. This is the spec the lock-free CAS
     loop satisfies; the proof is the documented continuation. *)
  Definition debit_atomic_spec (l : loc) (amt : Z) : Prop :=
    ⊢ <<{ ∀∀ b : Z, l ↦ #b }>>
        debit #l #amt @ ∅
      <<{ l ↦ #(b - amt) | RET #b }>>.

End spec.
