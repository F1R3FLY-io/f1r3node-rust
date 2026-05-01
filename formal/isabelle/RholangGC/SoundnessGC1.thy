(*
  SoundnessGC1.thy --- soundness of the escape + one-sided algorithm.

  States: every name in gc1(P) is garbage with respect to P.

  STATUS: Phase-1 stub.  The gc0 component reuses soundness_gc0 (proved
  in SoundnessGC0.thy).  The gc1-only component --- atoms u introduced
  by P's own `new` binders that don't escape and have one-sided usage
  --- is still under sorry.  Discharging it requires:

    1. A stronger reduction-relation invariant: an atom u that satisfies
       `retained_private P u` and (only_send_side P u OR only_recv_side
       P u OR a bundle-blocked refinement) cannot appear as a sync
       channel in any reachable configuration.

    2. Case analysis on the Comm rule that distinguishes datum-side and
       continuation-side atom origins, ruling out a "missing half" being
       supplied by K (since K cannot mention u by ctx_private) or
       through extrusion (since u doesn't escape).

    3. Bundle-aware analysis: when u only appears under bundle+ in
       sync-channel positions, only K can install matching senders ---
       and K cannot, so even Comms with persistent receives never fire.

  The proof of SoundnessGC0 establishes the necessary scaffolding
  (total_atoms invariant, single-step subset preservation, multi-step
  closure).  The GC1 case threads an additional invariant through
  configurations and uses the Comm rule's matchedness side-conditions.

  See docs/discoveries/rholang-gc-design.md \<section>3.2 for the algorithm
  and the example processes GC1 captures (e.g.\ `new x in {x!(0)}`).
*)

theory SoundnessGC1
  imports SoundnessGC0
begin

theorem soundness_gc1:
  assumes c_in_gc1: "c \<in> gc1 P"
  assumes safe: rholang_safe
  shows "is_garbage P c"
  sorry

text \<open>
  When the user's gc1 atom witness is itself a gc0 atom, soundness
  follows from \<open>soundness_gc0\<close>.  This corollary documents that
  containment.
\<close>

corollary soundness_gc1_via_gc0:
  assumes "c \<in> gc0 P"
  assumes safe: rholang_safe
  shows "is_garbage P c"
  using assms soundness_gc0 by blast

end
