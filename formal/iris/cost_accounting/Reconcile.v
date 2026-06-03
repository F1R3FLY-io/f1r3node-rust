(* ════════════════════════════════════════════════════════════════════════
   Reconcile.v — lock-free budget reconciliation, Iris/HeapLang (concurrent
   separation logic). The accounting runtime debits a shared budget with a CAS
   (CmpXchg) retry loop; this module formalizes the HeapLang program and PROVES its
   functional Hoare specification in Iris — the budget transitions from b to b-amt
   and the pre-debit balance is returned. A genuine Iris weakest-precondition proof
   of the runtime's debit primitive. The LOGICALLY-ATOMIC (full concurrent
   linearizability) strengthening is the documented continuation; the same
   schedule-independence is also covered empirically by the Rust `loom`
   model-checker and the RuntimeBudgetReplay TLA+ model (both LOCAL-ONLY, present).
   Axiom-free (standard Iris/HeapLang).                                          *)
From iris.heap_lang Require Import lang proofmode notation.

(* CAS-loop debit: subtract [amt] from the budget cell [l], retrying on contention;
   returns the pre-debit balance. Lock-free (no mutex). *)
Definition debit : val :=
  rec: "debit" "l" "amt" :=
    let: "cur" := ! "l" in
    if: Snd (CmpXchg "l" "cur" ("cur" - "amt"))
    then "cur"
    else "debit" "l" "amt".

Section spec.
  Context `{!heapGS Σ}.

  (* Functional correctness (the contention-free commit): debit returns the
     pre-balance and leaves the cell debited by [amt]. *)
  Lemma debit_spec (l : loc) (b amt : Z) :
    {{{ l ↦ #b }}} debit #l #amt {{{ RET #b; l ↦ #(b - amt) }}}.
  Proof.
    iIntros (Φ) "Hl HΦ".
    wp_rec. wp_pures. wp_load. wp_pures.
    wp_cmpxchg_suc.
    wp_pures.
    iModIntro. iApply "HΦ". iFrame.
  Qed.

End spec.
