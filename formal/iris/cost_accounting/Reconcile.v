(* ════════════════════════════════════════════════════════════════════════
   Reconcile.v — lock-free budget reconciliation, Iris/HeapLang (concurrent
   separation logic). The accounting runtime debits a shared budget with a CAS
   (CmpXchg) retry loop. This module formalizes the HeapLang program and PROVES, in
   Iris:
     - debit_spec        : the sequential / contention-free Hoare triple;
     - debit_atomic_spec : the LOGICALLY-ATOMIC triple — debit is LINEARIZABLE, its
                           effect (b ↦ b-amt, returning the pre-balance b) occurs
                           atomically at a single commit point EVEN UNDER CONCURRENT
                           INTERFERENCE on the cell (the CAS-fail branch retries via
                           Löb induction). This is the schedule-independence the
                           Rust `loom` model-checker and the RuntimeBudgetReplay
                           TLA+ model establish empirically; here it is a closed Iris
                           proof. Mirrors the standard atomic-CAS pattern
                           (iris.heap_lang.lib.increment.incr_phy_spec). Axiom-free. *)
From iris.base_logic.lib Require Import invariants.
From iris.program_logic Require Export atomic.
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

  (* Sequential (contention-free) correctness. *)
  Lemma debit_spec (l : loc) (b amt : Z) :
    {{{ l ↦ #b }}} debit #l #amt {{{ RET #b; l ↦ #(b - amt) }}}.
  Proof.
    iIntros (Φ) "Hl HΦ".
    wp_rec. wp_pures. wp_load. wp_pures.
    wp_cmpxchg_suc.
    wp_pures.
    iModIntro. iApply "HΦ". iFrame.
  Qed.

  (* Logically-atomic (linearizable) specification: the debit appears to take
     effect atomically at one commit point, under arbitrary concurrent interference
     on [l]. *)
  Lemma debit_atomic_spec (l : loc) (amt : Z) :
    ⊢ <<{ ∀∀ (b : Z), l ↦ #b }>> debit #l #amt @ ∅ <<{ l ↦ #(b - amt) | RET #b }>>.
  Proof.
    iIntros (Φ) "AU". iLöb as "IH". wp_rec. wp_pures.
    wp_bind (! _)%E. iMod "AU" as (v) "[Hl [Hclose _]]".
    wp_load. iMod ("Hclose" with "Hl") as "AU".
    iModIntro. wp_pures.
    wp_bind (CmpXchg _ _ _)%E. iMod "AU" as (w) "[Hl Hclose]".
    destruct (decide (#v = #w)) as [[= ->] | Hx].
    - wp_cmpxchg_suc. iDestruct "Hclose" as "[_ Hclose]".
      iMod ("Hclose" with "Hl") as "HΦ".
      iModIntro. wp_pures. done.
    - wp_cmpxchg_fail. iDestruct "Hclose" as "[Hclose _]".
      iMod ("Hclose" with "Hl") as "AU".
      iModIntro. wp_pures. iApply "IH". done.
  Qed.

End spec.
