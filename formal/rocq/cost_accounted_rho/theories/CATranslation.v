(* ════════════════════════════════════════════════════════════════════════
   CATranslation.v — native source-to-source translation to pure rho (Stage 4).

   The native four-sort cost-accounted SOURCE (caproc/caname/signed_term) erases
   into the UNCHANGED pure rho `proc`/`name` of RhoSyntax (the carrier split,
   DR-21).  Where the old `Translation.S_tr` translated a `system` whose signed
   body was ALREADY a pure proc, the native translation is mutually recursive —
   it descends through the native process structure and its signed-term
   continuations:

     - caname_tr : @T ↦ @(st_tr T);  bound var ↦ NVar (1:1 with the source).
     - p_tr      : for(x){T} ↦ for(N){st_tr T};  x!(U) ↦ N!(st_tr U);  *x, |, 0.
     - st_tr     : {P}_s ↦ the fuel gate  for(t ← N⟦s⟧){ p_tr P | *t }  (the body
                   lifted past the gate binder(s), exactly as the old P_tr);
                   T∥U ↦ st_tr T | st_tr U;  S ↦ T_tr S.

   The signature/token translations N_tr / T_tr are reused verbatim (sig/token
   are shared with the old model). This module gives the translation + its
   closedness; faithfulness/bisimulation (the simulation theorems) build on it.
   The hash_process/ground_process Section hypotheses are the audited ones (they
   surface in Print Assumptions exactly as for the old Translation). Axiom-free
   modulo those Section hypotheses.                                            *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
Import ListNotations.
From CostAccountedRho Require Import RhoSyntax.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.

Section CATranslationSec.

(* The audited Section hypotheses (mirroring Translation.v). *)
Variable hash_process : list bool -> proc.
Hypothesis hash_process_injective :
  forall b1 b2, hash_process b1 = hash_process b2 -> b1 = b2.
Hypothesis hash_process_closed : forall bs, closed_proc (hash_process bs).
Variable ground_process : list bool -> proc.
Hypothesis ground_process_injective :
  forall b1 b2, ground_process b1 = ground_process b2 -> b1 = b2.
Hypothesis ground_process_closed : forall bs, closed_proc (ground_process bs).
Hypothesis ground_hash_disjoint :
  forall b1 b2, ground_process b1 <> hash_process b2.

(* ── signature + token translation (sig/token shared; same as old model) ── *)

Fixpoint N_tr (s : sig) : name :=
  match s with
  | SUnit       => Quote PNil
  | SGround bs  => Quote (ground_process bs)
  | SQuote bs   => Quote (hash_process bs)
  | SAnd s1 s2  => Quote (PPar (PDeref (N_tr s1)) (PDeref (N_tr s2)))
  end.

Fixpoint T_tr (t : token) : proc :=
  match t with
  | TUnit       => PNil
  | TGate s t'  => POutput (N_tr s) (T_tr t')
  end.

(* An N-ary join erases to N nested unary for-comprehensions (the body lifted
   past all N binders): for(y1<-x1 & … & yN<-xN){T} ↦ for(n1){…for(nN){body}}. *)
Fixpoint iter_input (ns : list name) (body : proc) : proc :=
  match ns with
  | nil        => body
  | cons n ns' => PInput n (iter_input ns' body)
  end.

(* ── native mutual translation: caproc / caname / signed_term ↦ pure rho ── *)

Fixpoint p_tr (P : caproc) : proc :=
  match P with
  | CPNil        => PNil
  | CPInput x T  => PInput (caname_tr x) (st_tr T)
  | CPOutput x U => POutput (caname_tr x) (st_tr U)
  | CPPar P1 P2  => PPar (p_tr P1) (p_tr P2)
  | CPDeref x    => PDeref (caname_tr x)
  | CPJoin xs T  => iter_input (map caname_tr xs) (lift_proc (length xs) 0 (st_tr T))
  end
with caname_tr (x : caname) : name :=
  match x with
  | CQuote T => Quote (st_tr T)
  | CNVar k  => NVar k
  end
with st_tr (T : signed_term) : proc :=
  match T with
  | STSigned P s =>
      match s with
      | SAnd s1 s2 =>
          (* compound fuel gate: outer on N⟦s1⟧, inner on N⟦s2⟧; body lifted
             past BOTH gate binders, both payloads released (mirrors old P_tr). *)
          PInput (N_tr s1)
            (PInput (N_tr s2)
              (PPar (lift_proc 2 0 (p_tr P))
                    (PPar (PDeref (NVar 1)) (PDeref (NVar 0)))))
      | _ =>
          (* atomic fuel gate: for(t ← N⟦s⟧){ p_tr P | *t }, body lifted by 1. *)
          PInput (N_tr s)
            (PPar (lift_proc 1 0 (p_tr P)) (PDeref (NVar 0)))
      end
  | STPar T1 T2 => PPar (st_tr T1) (st_tr T2)
  | STStack t   => T_tr t
  end.

(* ── definitional unfolding lemmas (referenced by faithfulness) ─────────── *)

Lemma st_tr_par : forall T1 T2, st_tr (STPar T1 T2) = PPar (st_tr T1) (st_tr T2).
Proof. reflexivity. Qed.

Lemma st_tr_stack : forall t, st_tr (STStack t) = T_tr t.
Proof. reflexivity. Qed.

Lemma st_tr_signed_and : forall P s1 s2,
  st_tr (STSigned P (SAnd s1 s2)) =
    PInput (N_tr s1) (PInput (N_tr s2)
      (PPar (lift_proc 2 0 (p_tr P)) (PPar (PDeref (NVar 1)) (PDeref (NVar 0))))).
Proof. reflexivity. Qed.

Lemma st_tr_signed_unit : forall P,
  st_tr (STSigned P SUnit) =
    PInput (N_tr SUnit) (PPar (lift_proc 1 0 (p_tr P)) (PDeref (NVar 0))).
Proof. reflexivity. Qed.

Lemma p_tr_par : forall P1 P2, p_tr (CPPar P1 P2) = PPar (p_tr P1) (p_tr P2).
Proof. reflexivity. Qed.

(* ── closedness of the signature/token translation ──────────────────────── *)

Lemma N_tr_closed : forall s, closed_name (N_tr s).
Proof.
  induction s; simpl.
  - unfold closed_name; simpl; exact I.
  - apply closed_Quote, ground_process_closed.
  - apply closed_Quote, hash_process_closed.
  - apply closed_Quote. apply closed_PPar; apply closed_PDeref; assumption.
Qed.

Lemma T_tr_closed : forall t, closed_proc (T_tr t).
Proof.
  induction t; simpl.
  - apply closed_PNil.
  - apply closed_POutput; [ apply N_tr_closed | assumption ].
Qed.

End CATranslationSec.
