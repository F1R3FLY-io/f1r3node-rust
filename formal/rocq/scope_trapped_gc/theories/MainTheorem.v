(* ═══════════════════════════════════════════════════════════════════════════
   MainTheorem.v — capstones for CLAIM-RHOLANG-001

   Closed, fully generalized statements (the Section variables of
   ScopeTrapped.v — Lit, lit_eq_dec, trapped_lit, N — become premises
   of each term). The check script asserts each is "Closed under the
   global context".

   Capstone → claim mapping:
     rholang_scope_trapped_invariant  → C1 (invariant preservation under
                                            the reflexive-transitive step
                                            from `subst 0 N body`)
     rholang_scope_trapped_sweep_safe → C2 (sweep preserves the invariant)
   C3 (classifier determinism, locally_free cache correctness) is by
   construction: `scope_trapped` is a pure structural function; the
   cache correctness obligation remains Rust-side.
   S2 (fresh-name uniqueness) is by construction: Lit, trapped_lit, and
   N are universal Section Variables — the capstone holds for every
   instantiation.
   ═══════════════════════════════════════════════════════════════════════ *)

From ScopeTrappedGC Require Import ScopeTrapped.

Definition rholang_scope_trapped_invariant := @main_scope_trapped_safe.
Definition rholang_scope_trapped_sweep_safe := @sweep_preserves_inv.
