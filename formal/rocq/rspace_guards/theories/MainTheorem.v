(* ═══════════════════════════════════════════════════════════════════════════
   MainTheorem.v — capstones for CLAIM-RSPACE-001

   Closed, fully generalized statements (the Section variables of
   GuardParity.v — Data, Guard, guard_eval — become premises of each term).
   The check script asserts each is "Closed under the global context".

   Capstone → claim mapping:
     rspace_first_match_guard    → C1 (produce site, space_matcher.rs:161)
     rspace_play_guard_complete  → C1 (all play COMMs guard-checked)
     rspace_replay_log_gated     → D2 (replay commits only logged op ids)
     rspace_replay_equivalent    → C2 (replay of a play log = play COMMs)
     rspace_replay_guard_complete→ C2∘C1 (replayed COMMs all passed guards)
   C3 (guard determinism, bind-order agreement) is by construction: one
   shared [guard_eval] premise serves both play_from and replay_from.
   D1 (install never commits) is by construction of Op: OpInstall has no
   commit branch in either play_from or replay_from.
   ═══════════════════════════════════════════════════════════════════════ *)

From RSpaceGuards Require Import GuardParity.

Definition rspace_first_match_guard := first_match_guard_passes.
Definition rspace_play_guard_complete := play_guard_complete.
Definition rspace_replay_log_gated := replay_log_gated.
Definition rspace_replay_equivalent := replay_equiv.
Definition rspace_replay_guard_complete := replay_guard_complete.
