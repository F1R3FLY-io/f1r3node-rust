/-
  CostAccountedRho.JoinConservation — Lean 4 (core, Mathlib-free) cross-witness for
  the N-ary join authority-conservation algebra (spec §4.8 Prop 4.7 / §4.8.5),
  mirroring the Rocq CAJoinConservation development. Authorities are the free SAnd
  tensor over atoms; `combinedKey` mirrors the Rocq fold. Proves the no-weakening
  corollary (a non-trivial combined key strictly exceeds the receiver authority
  alone). Dependency-free so `lake build` stays fully offline (the full multiset
  conservation is the Isabelle/HOL leg, which has HOL-Library.Multiset). Kept in a
  dedicated sub-namespace so its `Authority` type does not collide with the
  step-calculus `CostAccountedRho.Sig`.
-/

namespace CostAccountedRho.JoinConservation

inductive Authority where
  | leaf : Nat → Authority
  | and  : Authority → Authority → Authority

def authSize : Authority → Nat
  | .leaf _   => 1
  | .and a b  => authSize a + authSize b

theorem authSize_pos (s : Authority) : 1 ≤ authSize s := by
  induction s with
  | leaf _ => simp [authSize]
  | and a b iha _ => simp [authSize]; omega

def combinedKey (s1 : Authority) : List Authority → Authority
  | []       => s1
  | t :: ts  => .and (combinedKey s1 ts) t

theorem key_ge (s1 : Authority) (ts : List Authority) :
    authSize s1 ≤ authSize (combinedKey s1 ts) := by
  induction ts with
  | nil => simp [combinedKey]
  | cons t ts ih => simp [combinedKey, authSize]; omega

/-- No-weakening: a fired non-trivial join strictly exceeds the receiver
    authority alone — the sender authorities cannot be silently dropped. -/
theorem join_no_weakening (s1 t : Authority) (ts : List Authority) :
    authSize s1 < authSize (combinedKey s1 (t :: ts)) := by
  have h1 := key_ge s1 ts
  have h2 := authSize_pos t
  simp only [combinedKey, authSize]
  omega

end CostAccountedRho.JoinConservation
