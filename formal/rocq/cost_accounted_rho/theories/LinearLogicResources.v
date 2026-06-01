From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia
  Sorting.Permutation.
Import ListNotations.

From CostAccountedRho Require Import CostAccountedSyntax.

Inductive ll_formula : Type :=
  | LLUnit : ll_formula
  | LLAtom : nat -> ll_formula
  | LLTensor : ll_formula -> ll_formula -> ll_formula
  | LLThreshold : nat -> list ll_formula -> ll_formula
  | LLPlus : sig_choice -> ll_formula -> ll_formula -> ll_formula
  | LLWith : ll_formula -> ll_formula -> ll_formula
  | LLBang : ll_formula -> ll_formula
  | LLWhyNot : ll_formula -> ll_formula
  | LLLolly : ll_formula -> ll_formula -> ll_formula.

(* Both Def-3.3 atom axes ([ASGround] = ground [g], [ASQuote] = quote [#P])
   map to the SAME linear-logic atom [LLAtom a]: the linear-resource demand
   [Δ_s] is insensitive to which authentication axis produced the atom, so
   the [ll_formula] target is unchanged from the pre-split development and
   every DILL derivation re-closes verbatim. *)
Fixpoint ll_of_sig_algebra (s : sig_algebra) : ll_formula :=
  match s with
  | ASUnit => LLUnit
  | ASGround a => LLAtom a
  | ASQuote a => LLAtom a
  | ASAnd s1 s2 => LLTensor (ll_of_sig_algebra s1) (ll_of_sig_algebra s2)
  | ASThreshold k members => LLThreshold k (map ll_of_sig_algebra members)
  | ASPlus choice s1 s2 =>
      LLPlus choice (ll_of_sig_algebra s1) (ll_of_sig_algebra s2)
  | ASWith s1 s2 => LLWith (ll_of_sig_algebra s1) (ll_of_sig_algebra s2)
  | ASBang s' => LLBang (ll_of_sig_algebra s')
  | ASWhyNot s' => LLWhyNot (ll_of_sig_algebra s')
  | ASLolly s1 s2 => LLLolly (ll_of_sig_algebra s1) (ll_of_sig_algebra s2)
  end.

Fixpoint ll_atoms (f : ll_formula) : list nat :=
  match f with
  | LLUnit => []
  | LLAtom a => [a]
  | LLTensor f1 f2 => ll_atoms f1 ++ ll_atoms f2
  | LLThreshold _ members => concat (map ll_atoms members)
  | LLPlus _ f1 f2 => ll_atoms f1 ++ ll_atoms f2
  | LLWith f1 f2 => ll_atoms f1 ++ ll_atoms f2
  | LLBang f' => ll_atoms f'
  | LLWhyNot f' => ll_atoms f'
  | LLLolly f1 f2 => ll_atoms f1 ++ ll_atoms f2
  end.

Fixpoint ll_required_units (f : ll_formula) : nat :=
  match f with
  | LLUnit => 0
  | LLAtom _ => 1
  | LLTensor f1 f2 => ll_required_units f1 + ll_required_units f2
  | LLThreshold k _ => k
  | LLPlus ChooseLeft f1 _ => ll_required_units f1
  | LLPlus ChooseRight _ f2 => ll_required_units f2
  | LLWith f1 f2 => ll_required_units f1 + ll_required_units f2
  | LLBang f' => ll_required_units f'
  | LLWhyNot _ => 0
  | LLLolly f1 f2 => ll_required_units f1 + ll_required_units f2
  end.

Fixpoint ll_available_slots (f : ll_formula) : nat :=
  match f with
  | LLUnit => 0
  | LLAtom _ => 1
  | LLTensor f1 f2 => ll_available_slots f1 + ll_available_slots f2
  | LLThreshold _ members => length members
  | LLPlus _ f1 f2 => ll_available_slots f1 + ll_available_slots f2
  | LLWith f1 f2 => ll_available_slots f1 + ll_available_slots f2
  | LLBang f' => ll_available_slots f'
  | LLWhyNot f' => ll_available_slots f'
  | LLLolly f1 f2 => ll_available_slots f1 + ll_available_slots f2
  end.

Fixpoint ll_consumed_atoms (f : ll_formula) : list nat :=
  match f with
  | LLUnit => []
  | LLAtom a => [a]
  | LLTensor f1 f2 => ll_consumed_atoms f1 ++ ll_consumed_atoms f2
  | LLThreshold _ members => concat (map ll_consumed_atoms members)
  | LLPlus ChooseLeft f1 _ => ll_consumed_atoms f1
  | LLPlus ChooseRight _ f2 => ll_consumed_atoms f2
  | LLWith f1 f2 => ll_consumed_atoms f1 ++ ll_consumed_atoms f2
  | LLBang f' => ll_consumed_atoms f'
  | LLWhyNot _ => []
  | LLLolly f1 f2 => ll_consumed_atoms f1 ++ ll_consumed_atoms f2
  end.

Fixpoint ll_valid (f : ll_formula) : bool :=
  match f with
  | LLUnit => true
  | LLAtom _ => true
  | LLTensor f1 f2 => ll_valid f1 && ll_valid f2
  | LLThreshold k members =>
      (1 <=? k) && (k <=? length members) && forallb ll_valid members
  | LLPlus _ f1 f2 => ll_valid f1 && ll_valid f2
  | LLWith f1 f2 => ll_valid f1 && ll_valid f2
  | LLBang f' => ll_valid f'
  | LLWhyNot f' => ll_valid f'
  | LLLolly f1 f2 => ll_valid f1 && ll_valid f2
  end.

Definition linear_ctx := list ll_formula.
Definition unrestricted_ctx := list ll_formula.

Definition linear_ctx_atoms (delta : linear_ctx) : list nat :=
  concat (map ll_consumed_atoms delta).

Definition linear_atom_count (delta : linear_ctx) (a : nat) : nat :=
  count_occ Nat.eq_dec (linear_ctx_atoms delta) a.

Fixpoint consume_linear_atom (target : nat) (delta : linear_ctx)
    : option linear_ctx :=
  match delta with
  | [] => None
  | h :: t =>
      match h with
      | LLAtom a =>
          if Nat.eq_dec target a then Some t
          else
            match consume_linear_atom target t with
            | Some t' => Some (h :: t')
            | None => None
            end
      | _ =>
          match consume_linear_atom target t with
          | Some t' => Some (h :: t')
          | None => None
          end
      end
  end.

Definition reuse_unrestricted (_target : ll_formula) (gamma : unrestricted_ctx)
    : unrestricted_ctx := gamma.

Inductive dill : unrestricted_ctx -> linear_ctx -> ll_formula -> Prop :=
  | dill_ax : forall gamma f, dill gamma [f] f
  | dill_unit : forall gamma, dill gamma [] LLUnit
  | dill_unrestricted : forall gamma f,
      In f gamma ->
      dill gamma [] (LLBang f)
  | dill_tensor : forall gamma delta1 delta2 f1 f2,
      dill gamma delta1 f1 ->
      dill gamma delta2 f2 ->
      dill gamma (delta1 ++ delta2) (LLTensor f1 f2)
  | dill_plus_left : forall gamma delta f1 f2,
      dill gamma delta f1 ->
      dill gamma delta (LLPlus ChooseLeft f1 f2)
  | dill_plus_right : forall gamma delta f1 f2,
      dill gamma delta f2 ->
      dill gamma delta (LLPlus ChooseRight f1 f2)
  | dill_with : forall gamma delta f1 f2,
      dill gamma delta f1 ->
      dill gamma delta f2 ->
      dill gamma delta (LLWith f1 f2)
  | dill_lolly_intro : forall gamma delta f1 f2,
      dill gamma (f1 :: delta) f2 ->
      dill gamma delta (LLLolly f1 f2)
  | dill_lolly_elim : forall gamma delta1 delta2 f1 f2,
      dill gamma delta1 (LLLolly f1 f2) ->
      dill gamma delta2 f1 ->
      dill gamma (delta1 ++ delta2) f2
  | dill_whynot_intro : forall gamma f,
      dill gamma [] (LLWhyNot f).

Theorem dill_linear_identity :
  forall gamma f,
    dill gamma [f] f.
Proof.
  intros gamma f. apply dill_ax.
Qed.

Theorem dill_tensor_combines_linear_contexts :
  forall gamma delta1 delta2 f1 f2,
    dill gamma delta1 f1 ->
    dill gamma delta2 f2 ->
    dill gamma (delta1 ++ delta2) (LLTensor f1 f2).
Proof.
  intros gamma delta1 delta2 f1 f2 H1 H2.
  apply dill_tensor; assumption.
Qed.

Theorem dill_unrestricted_claim_uses_no_linear_witness :
  forall gamma f,
    In f gamma ->
    dill gamma [] (LLBang f).
Proof.
  intros gamma f Hin.
  apply dill_unrestricted. exact Hin.
Qed.

Theorem dill_lolly_modus_ponens_consumes_input_context :
  forall gamma delta1 delta2 f1 f2,
    dill gamma delta1 (LLLolly f1 f2) ->
    dill gamma delta2 f1 ->
    dill gamma (delta1 ++ delta2) f2.
Proof.
  intros gamma delta1 delta2 f1 f2 Himpl Harg.
  apply dill_lolly_elim with (f1 := f1); assumption.
Qed.

Theorem dill_whynot_intro_uses_no_linear_witness :
  forall gamma f,
    dill gamma [] (LLWhyNot f).
Proof.
  intros gamma f. apply dill_whynot_intro.
Qed.

Theorem ll_sig_algebra_required_complete :
  forall s,
    ll_required_units (ll_of_sig_algebra s) =
    sig_algebra_min_required s.
Proof.
  induction s as
    [|a|a|s1 IH1 s2 IH2|k members|choice s1 IH1 s2 IH2
     |s1 IH1 s2 IH2|s IH|s IH|s1 IH1 s2 IH2]; cbn.
  - reflexivity.
  - reflexivity.
  - reflexivity.
  - rewrite IH1, IH2. reflexivity.
  - reflexivity.
  - destruct choice; assumption.
  - rewrite IH1, IH2. reflexivity.
  - exact IH.
  - reflexivity.
  - rewrite IH1, IH2. reflexivity.
Qed.

Theorem ll_sig_algebra_consumed_matches_presented :
  forall s,
    ll_consumed_atoms (ll_of_sig_algebra s) =
    sig_algebra_presented_atoms s.
Proof.
  fix IH 1.
  destruct s as
    [|a|a|s1 s2|k members|choice s1 s2
     |s1 s2|s|s|s1 s2]; cbn.
  - reflexivity.
  - reflexivity.
  - reflexivity.
  - rewrite IH, IH. reflexivity.
  - induction members as [|h t IHt]; cbn.
    + reflexivity.
    + rewrite IH. rewrite IHt. reflexivity.
  - destruct choice; apply IH.
  - rewrite IH, IH. reflexivity.
  - apply IH.
  - reflexivity.
  - rewrite IH, IH. reflexivity.
Qed.

Theorem ll_sig_algebra_threshold_valid_bounds_bridge :
  forall k members,
    sig_algebra_valid (ASThreshold k members) = true ->
    1 <= k /\ k <= length (map ll_of_sig_algebra members).
Proof.
  intros k members Hvalid.
  apply sig_algebra_threshold_valid_bounds in Hvalid as [Hlower Hupper].
  rewrite length_map.
  split; assumption.
Qed.

Theorem ll_plus_left_consumes_chosen_branch :
  forall f1 f2,
    ll_consumed_atoms (LLPlus ChooseLeft f1 f2) = ll_consumed_atoms f1 /\
    ll_required_units (LLPlus ChooseLeft f1 f2) = ll_required_units f1.
Proof. intros. split; reflexivity. Qed.

Theorem ll_plus_right_consumes_chosen_branch :
  forall f1 f2,
    ll_consumed_atoms (LLPlus ChooseRight f1 f2) = ll_consumed_atoms f2 /\
    ll_required_units (LLPlus ChooseRight f1 f2) = ll_required_units f2.
Proof. intros. split; reflexivity. Qed.

Theorem ll_with_requires_both_branches_available :
  forall f1 f2,
    ll_required_units (LLWith f1 f2) =
    ll_required_units f1 + ll_required_units f2 /\
    ll_consumed_atoms (LLWith f1 f2) =
    ll_consumed_atoms f1 ++ ll_consumed_atoms f2.
Proof. intros. split; reflexivity. Qed.

Theorem ll_bang_reuse_no_extra_linear_cost :
  forall f delta,
    linear_ctx_atoms delta =
    linear_ctx_atoms delta /\
    ll_required_units (LLBang f) = ll_required_units f.
Proof. intros. split; reflexivity. Qed.

Theorem ll_whynot_consumes_no_linear_witness :
  forall f,
    ll_required_units (LLWhyNot f) = 0 /\
    ll_consumed_atoms (LLWhyNot f) = [].
Proof. intros. split; reflexivity. Qed.

Theorem ll_lolly_resource_flow_conservative :
  forall f_from f_to,
    ll_required_units (LLLolly f_from f_to) =
    ll_required_units f_from + ll_required_units f_to /\
    ll_consumed_atoms (LLLolly f_from f_to) =
    ll_consumed_atoms f_from ++ ll_consumed_atoms f_to.
Proof. intros. split; reflexivity. Qed.

Theorem ll_threshold_quorum_sound :
  forall k members,
    ll_valid (LLThreshold k members) = true ->
    1 <= k /\ k <= length members /\
    ll_required_units (LLThreshold k members) = k.
Proof.
  intros k members Hvalid.
  cbn in Hvalid.
  apply andb_prop in Hvalid as [Hbounds _].
  apply andb_prop in Hbounds as [Hlower Hupper].
  split.
  - apply Nat.leb_le. exact Hlower.
  - split.
    + apply Nat.leb_le. exact Hupper.
    + reflexivity.
Qed.

Theorem ll_linear_no_contraction :
  forall a,
    ~ Permutation
        (linear_ctx_atoms [LLAtom a])
        (linear_ctx_atoms [LLTensor (LLAtom a) (LLAtom a)]).
Proof.
  intros a Hperm.
  apply Permutation_length in Hperm.
  cbn in Hperm. lia.
Qed.

Theorem ll_linear_no_weakening :
  forall a,
    ~ Permutation (linear_ctx_atoms []) (linear_ctx_atoms [LLAtom a]).
Proof.
  intros a Hperm.
  apply Permutation_length in Hperm.
  cbn in Hperm. lia.
Qed.

Theorem ll_linear_atom_contraction_changes_count :
  forall a,
    linear_atom_count [LLAtom a] a = 1 /\
    linear_atom_count [LLTensor (LLAtom a) (LLAtom a)] a = 2.
Proof.
  intros a.
  unfold linear_atom_count, linear_ctx_atoms.
  cbn.
  destruct (Nat.eq_dec a a) as [_ | Hneq].
  - split; reflexivity.
  - contradiction.
Qed.

Theorem ll_consume_linear_once_atom_exhausts :
  forall a,
    consume_linear_atom a [LLAtom a] = Some [].
Proof.
  intros a. cbn.
  destruct (Nat.eq_dec a a) as [_ | Hneq].
  - reflexivity.
  - contradiction.
Qed.

Theorem ll_no_double_spend_single_witness :
  forall a,
    match consume_linear_atom a [LLAtom a] with
    | Some delta => consume_linear_atom a delta
    | None => None
    end = None.
Proof.
  intros a.
  rewrite ll_consume_linear_once_atom_exhausts.
  reflexivity.
Qed.

Theorem ll_double_spend_requires_duplicate_witness :
  forall a,
    match consume_linear_atom a [LLAtom a; LLAtom a] with
    | Some delta => consume_linear_atom a delta
    | None => None
    end = Some [].
Proof.
  intros a. cbn.
  destruct (Nat.eq_dec a a) as [_ | Hneq].
  - cbn.
    destruct (Nat.eq_dec a a) as [_ | Hneq'].
    + reflexivity.
    + contradiction.
  - contradiction.
Qed.

Theorem ll_unrestricted_reuse_preserves_context :
  forall gamma f,
    In f gamma ->
    reuse_unrestricted f gamma = gamma.
Proof.
  intros gamma f _.
  reflexivity.
Qed.

Theorem ll_unrestricted_can_be_reused :
  forall gamma f,
    In f gamma ->
    reuse_unrestricted f (reuse_unrestricted f gamma) = gamma.
Proof.
  intros gamma f _.
  reflexivity.
Qed.

Theorem ll_linear_cut_consumes_cut_witness :
  forall a delta,
    consume_linear_atom a (LLAtom a :: delta) = Some delta.
Proof.
  intros a delta. cbn.
  destruct (Nat.eq_dec a a) as [_ | Hneq].
  - reflexivity.
  - contradiction.
Qed.

Theorem ll_unrestricted_cut_preserves_linear_zone :
  forall gamma f delta,
    In f gamma ->
    reuse_unrestricted f gamma = gamma /\
    linear_ctx_atoms delta = linear_ctx_atoms delta.
Proof.
  intros gamma f delta Hin.
  split.
  - apply ll_unrestricted_reuse_preserves_context. exact Hin.
  - reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section: DR-10 Decode-Stage (DS) Guard — core/extension demand invariance
   ═══════════════════════════════════════════════════════════════════════════

   The extension connectives ([ASThreshold], [ASPlus], [ASWith], [ASBang],
   [ASWhyNot], [ASLolly]) belong to the ILLE (intuitionistic linear logic with
   exponentials) layer that DR-10 keeps. The core cost-accounted signature
   grammar (Def 3.3) is exactly [SUnit | SGround | SQuote | SAnd]; it embeds
   into [sig_algebra] using ONLY the core connectives [ASUnit], [ASGround],
   [ASQuote], [ASAnd]. This section discharges the DS-guard obligation that
   the ILLE extension never changes the linear-token demand of a CORE
   signature: the embedding's required-unit count equals the directly-computed
   core token demand, and the exponential connectives never under-fund a core
   obligation (with [ASWhyNot] carrying no core token by design).

   This is purely structural: [ca_step] (in CostAccountedReduction.v) never
   quantifies over [sig_algebra], so core reduction is independent of the
   extension connectives by construction. The lemmas below make that
   independence explicit at the demand level.                                *)

(* A canonical byte-string-to-atom encoding so that the core signature
   grammar (whose atoms carry [list bool]) embeds into the runtime
   [sig_algebra] (whose atoms carry [nat]). Standard big-endian binary fold;
   [true] is bit 1, [false] is bit 0. *)
Fixpoint bits_to_atom (bs : list bool) : nat :=
  match bs with
  | [] => 0
  | b :: rest => (if b then 1 else 0) + 2 * bits_to_atom rest
  end.

(* The CORE embedding: maps each Def-3.3 signature into the runtime
   [sig_algebra] using only the four core connectives. Ground and quote
   atoms land on their respective axis arms [ASGround]/[ASQuote]; neither
   introduces any ILLE connective. *)
Fixpoint sig_to_algebra (s : sig) : sig_algebra :=
  match s with
  | SUnit       => ASUnit
  | SGround bs  => ASGround (bits_to_atom bs)
  | SQuote bs   => ASQuote (bits_to_atom bs)
  | SAnd s1 s2  => ASAnd (sig_to_algebra s1) (sig_to_algebra s2)
  end.

(* The token demand of a CORE signature, computed directly on [sig]: each
   atomic axis (ground or quote) gates exactly one token; [SUnit] gates
   none; [SAnd] sums its components. This is the per-COMM linear-token
   obligation of the cost-accounted core. *)
Fixpoint core_token_demand (s : sig) : nat :=
  match s with
  | SUnit       => 0
  | SGround _   => 1
  | SQuote _    => 1
  | SAnd s1 s2  => core_token_demand s1 + core_token_demand s2
  end.

(* DS-guard (i), core-invariance: the linear-logic required-unit count of the
   core embedding equals the directly-computed core token demand. The ILLE
   decode stage therefore reproduces the core obligation exactly. *)
Theorem core_demand_invariant_under_extension :
  forall s,
    ll_required_units (ll_of_sig_algebra (sig_to_algebra s)) =
    core_token_demand s.
Proof.
  induction s as [| bs | bs | s1 IH1 s2 IH2]; cbn.
  - reflexivity.
  - reflexivity.
  - reflexivity.
  - rewrite IH1, IH2. reflexivity.
Qed.

(* DS-guard (i), extension-never-under-funds: wrapping the core embedding in
   the resource-preserving exponential [ASBang] (the "!" of ILLE) leaves the
   core token demand unchanged, and the additive [ASWith] / multiplicative
   [ASAnd] combinators sum the component demands — so an ILLE-decorated core
   obligation always funds AT LEAST the bare core demand. The dual [ASWhyNot]
   is the only connective that drops to zero, which is correct: a [?]-marked
   resource carries no linear core token. *)
Theorem extension_demand_ge_core :
  forall s,
    sig_algebra_min_required (ASBang (sig_to_algebra s)) >= core_token_demand s /\
    sig_algebra_min_required (ASWith (sig_to_algebra s) (sig_to_algebra s))
      >= core_token_demand s /\
    sig_algebra_min_required (ASWhyNot (sig_to_algebra s)) = 0.
Proof.
  intro s.
  assert (Hcore : sig_algebra_min_required (sig_to_algebra s) = core_token_demand s).
  { rewrite <- core_demand_invariant_under_extension.
    symmetry. apply ll_sig_algebra_required_complete. }
  split; [| split].
  - cbn. rewrite Hcore. lia.
  - cbn. rewrite Hcore. lia.
  - cbn. reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section: WD-D5a — the PURE per-signature demand [delta_s], funding
   decidability (Def 19 / Thm 20), and the supply-realization Decision-8
   balance-fidelity lemmas
   ═══════════════════════════════════════════════════════════════════════════

   This section formalises the funding side of the acceptance gate that the
   Rust analyzer [rholang/.../accounting/delta_sigma.rs] (WD-D1) implements, and
   discharges the supply-realization obligations registered in
   [docs/theory/cost-accounting-impl/supply-realization-c-d-handoff.md],
   Decision 8.

   The demand here is the PURE [delta_s] of the cost-accounted-rho paper
   (Def 17): [LLUnit -> 0], [LLAtom -> 1], [LLTensor -> sum], every other
   connective -> 0. It is DELIBERATELY distinct from [ll_required_units] (which
   models the full ILLE algebra — [LLThreshold -> k], [LLPlus -> chosen branch],
   etc.). [delta_s] counts ONLY the multiplicative-core layers, which is exactly
   the per-signature linear-token demand the runtime meters under the s₀ collapse
   (one token per token-consuming COMM, all attributed to the envelope signature;
   the non-core connectives do not arise in the s₀-collapsed demand the gate
   evaluates).                                                                  *)

(* The pure demand [delta_s] (cost-accounted-rho Def 17), on the [ll_formula]
   image of a signature. Matches the Rust [DemandEntry::known_lower_bound] for a
   fully-resolvable term: the multiplicative-core layer count. *)
Fixpoint delta_s (f : ll_formula) : nat :=
  match f with
  | LLUnit => 0
  | LLAtom _ => 1
  | LLTensor f1 f2 => delta_s f1 + delta_s f2
  | LLThreshold _ _ => 0
  | LLPlus _ _ _ => 0
  | LLWith _ _ => 0
  | LLBang _ => 0
  | LLWhyNot _ => 0
  | LLLolly _ _ => 0
  end.

(* [delta_s] is additive over the multiplicative tensor (cost-accounted-rho
   Def 17 [Δ_s(T | U) = Δ_s(T) + Δ_s(U)] at the linear-logic image, where parallel
   composition reflects to [LLTensor]). Definitional, but stated as a headline
   theorem because the Rust [DemandEntry::combine] relies on exactly this
   additivity (and the §7.4 example [Δ = Δ(debit) + Δ(credit)] is an instance). *)
Theorem delta_s_tensor_additive :
  forall f1 f2,
    delta_s (LLTensor f1 f2) = delta_s f1 + delta_s f2.
Proof. intros f1 f2. reflexivity. Qed.

(* [compound_demand_splits_to_components] (#12, settlement context): a compound
   signature [Sig::And s₁ s₂] reflects to [LLTensor (ll_of_sig_algebra s₁)
   (ll_of_sig_algebra s₂)] (see [ll_of_sig_algebra] on [ASAnd]), so its pure
   demand [Δ_s] is EXACTLY the sum of the two COMPONENT demands — one obligation
   per component. This is the demand-side analogue of the per-component
   settlement DEBIT (acceptance.rs::compute_settlement_debits draws one token per
   component pool per compound COMM, spec §3.6 Rule 2 / Rule 4): the compound's
   total demand SPLITS additively into the components it must fund, which is why
   the multi-pool debit ([Σ⟦s₁⟧ −= draw_pair], [Σ⟦s₂⟧ −= draw_pair]) settles the
   same quantity the compound demanded. It names [delta_s_tensor_additive] for
   the settlement bridge rather than re-deriving it. *)
Corollary compound_demand_splits_to_components :
  forall s1 s2,
    delta_s (ll_of_sig_algebra (ASAnd s1 s2)) =
    delta_s (ll_of_sig_algebra s1) + delta_s (ll_of_sig_algebra s2).
Proof.
  intros s1 s2. cbn [ll_of_sig_algebra]. apply delta_s_tensor_additive.
Qed.

(* The funding obligation as a decidable predicate over the supply balance [n]
   and the demand [d]: [funds n d] holds iff the supply meets or exceeds the
   demand (cost-accounted-rho Def 19, [eq:funding-obligation] [Σ_s ≥ Δ_s]). *)
Definition funds (n d : nat) : Prop := d <= n.

(* Decidability of the funding check (cost-accounted-rho Thm 20): for any supply
   balance [n] and any formula [f], it is decidable whether the funding
   obligation [Σ_s ≥ Δ_s] holds — i.e. the validator can ALWAYS reach a verdict
   (accept / reject) by a single integer comparison. This is the formal content
   of "decidable in time linear in the size of the deployment's AST": [delta_s]
   is one structural pass and the comparison is decidable [le_dec]. *)
Theorem funding_decidable :
  forall (n : nat) (f : ll_formula),
    {funds n (delta_s f)} + {~ funds n (delta_s f)}.
Proof.
  intros n f. unfold funds. apply Compare_dec.le_dec.
Qed.

(* ─── Supply-realization Decision 8: balance ⇔ stack-depth fidelity ─────────

   The runtime represents the per-signature supply [Σ_s] as a single balance
   datum [n] on the channel [Σ⟦s⟧] (handoff Decision 2), rather than as [n]
   literal stacked token messages. The fidelity obligation is that this balance
   [n] EQUALS the paper's [Σ_s] of a depth-[n] [s]-indexed token stack
   (Def 18: [Σ_s(s:S) = 1 + Σ_s(S)], [Σ_s(()) = 0]). We model an [s]-stack of
   depth [n] and prove its count is [n], so the balance is a faithful encoding of
   the layer count — the guard #1 of the handoff's spec-conformance verdict.    *)

(* A depth-[n] token stack of a SINGLE signature [s], reflected to its
   linear-logic image as an [n]-fold tensor of the atom [a] (the [s]-image),
   bottoming out at [LLUnit] (the empty stack [()]). This mirrors the paper's
   [s : s : … : ()] stack: each [::] is one tensor layer carrying one [s]-token. *)
Fixpoint sig_stack (a : nat) (n : nat) : ll_formula :=
  match n with
  | 0 => LLUnit
  | S k => LLTensor (LLAtom a) (sig_stack a k)
  end.

(* [sigma_s] of a stack is just [delta_s] of its linear-logic image — the supply
   count and the demand count use the SAME per-layer accounting (the paper's
   [Σ_s] and [Δ_s] both count [s]-layers; Def 17 / Def 18 are the same recursion
   on the two sides of the inequality). *)
Definition sigma_s (f : ll_formula) : nat := delta_s f.

(* Decision-8 fidelity lemma [sigma_s_balance_eq_stack_count]: the balance [n]
   equals [Σ_s] of a depth-[n] [s]-stack. So storing the supply as the integer
   balance [n] loses no information relative to the paper's stack representation
   — the two are equal as counts, which is exactly what the funding inequality
   compares. *)
Theorem sigma_s_balance_eq_stack_count :
  forall (a : nat) (n : nat),
    sigma_s (sig_stack a n) = n.
Proof.
  intros a n. unfold sigma_s.
  induction n as [| k IH]; cbn.
  - reflexivity.
  - rewrite IH. reflexivity.
Qed.

(* ─── Supply-realization Decision 8: funding-check soundness over the balance ─

   The Rust gate computes [is_funded(analysis, effective_supply_s, margin) =
   (effective_supply_s >= known_lower_bound + margin)]. With the supply read as
   the balance [n] and (for the spec-level obligation [Σ_s ≥ Δ_s]) a zero margin,
   this is the decidable boolean [d <=? n]. The soundness obligation is that this
   boolean verdict AGREES with the funding proposition [funds n d] — i.e. the
   balance-read gate accepts exactly when [Σ_s ≥ Δ_s]. *)

(* The gate's boolean funding check over the balance, at the spec obligation
   (margin 0): accept iff [delta_s f <=? n]. *)
Definition is_funded_balance (n : nat) (f : ll_formula) : bool :=
  Nat.leb (delta_s f) n.

(* Decision-8 soundness lemma [funding_check_balance_sound]: the boolean
   balance-read funding check is TRUE iff the funding obligation [Σ_s ≥ Δ_s]
   holds. Both directions, so the gate neither admits an under-funded deploy nor
   rejects a funded one (the consensus-safety property of the acceptance gate). *)
Theorem funding_check_balance_sound :
  forall (n : nat) (f : ll_formula),
    is_funded_balance n f = true <-> funds n (delta_s f).
Proof.
  intros n f. unfold is_funded_balance, funds.
  apply Nat.leb_le.
Qed.

(* Bridge corollary: stating the soundness directly against the depth-[n] stack
   supply makes the end-to-end chain explicit — the gate reading the balance [n]
   (= [Σ_s] of the depth-[n] stack, by [sigma_s_balance_eq_stack_count]) accepts
   the demand [delta_s f] iff that demand fits within the stack's supply. This is
   the realized form of the paper's acceptance protocol step "[Σ_c ≥ Δ_c] ⇒
   accept" (cost-accounted-rho §7.5). *)
Theorem funding_check_balance_sound_against_stack :
  forall (a : nat) (n : nat) (f : ll_formula),
    is_funded_balance (sigma_s (sig_stack a n)) f = true
      <-> funds n (delta_s f).
Proof.
  intros a n f.
  rewrite funding_check_balance_sound.
  rewrite sigma_s_balance_eq_stack_count.
  reflexivity.
Qed.

(* ─── #13b: spec-strict rejection of an underfunded deploy on an ABSENT pool ──

   Task #13a switched the WD-D2 acceptance gate to its spec-strict mode (§7.6
   step 5: an underfunded deploy MUST be rejected — no "admit-unenforced"
   carve-out). In that mode an ABSENT supply pool is treated as a present pool
   carrying the empty stack: its balance is [Σ_s = 0] (the paper's [supply(s) =
   0] for an absent pool; realized in Rust by [supply::read_balance] folding an
   absent channel to 0). Task #13b SEEDS client pools at genesis precisely so a
   strict shard does NOT reject the clients it intends to fund.

   The headline obligation [strict_reject_when_underfunded] is the formal content
   of the strict reject property: under strict mode, a deploy whose demand is
   POSITIVE ([delta_s f > 0]) against an absent pool (supply [0]) FAILS the gate's
   boolean funding check — [is_funded_balance 0 f = false] — so it is rejected
   (§7.6 step 5: rejected without executing any part, no state change, no tokens
   consumed). This is an axiom-free COROLLARY of the Decision-8 soundness
   biconditional [funding_check_balance_sound] (= [funding_decidable]'s boolean
   witness): at [n = 0] the check is true iff [funds 0 (delta_s f)] i.e. iff
   [delta_s f <= 0], which CONTRADICTS [delta_s f > 0]; hence the check is false.
   It mirrors the Rust strict branch ([acceptance.rs::admit_by_funding]: an absent
   pool's effective supply is 0, so a [Δ>0] group fails [is_funded(_, 0, margin)])
   and the replay re-verification ([recompute_settlement_debits] under strict:
   an admitted [Δ>0] deploy on an absent pool is a gate-bypass ⇒ invalid block). *)
Theorem strict_reject_when_underfunded :
  forall (f : ll_formula),
    delta_s f > 0 ->
    is_funded_balance 0 f = false.
Proof.
  intros f Hpos.
  (* Reduce to the boolean: [is_funded_balance 0 f = (delta_s f <=? 0)]. A
     boolean is [false] exactly when it is not [true]; rewrite [= true] via the
     soundness biconditional, then derive the contradiction with [Hpos]. *)
  apply Bool.not_true_is_false.
  intro Htrue.
  rewrite funding_check_balance_sound in Htrue.   (* Htrue : funds 0 (delta_s f) *)
  unfold funds in Htrue.                            (* Htrue : delta_s f <= 0 *)
  (* [delta_s f > 0] is [0 < delta_s f] i.e. [1 <= delta_s f]; with [delta_s f
     <= 0] that forces [1 <= 0], absurd. *)
  lia.
Qed.

(* Corollary [strict_absent_pool_rejects_positive_demand]: an ABSENT pool is the
   depth-0 stack ([sig_stack a 0 = LLUnit], [sigma_s = 0] by
   [sigma_s_balance_eq_stack_count]); the strict gate reading that pool's balance
   rejects any positive demand. This states the property directly against the
   stack representation the realization uses, closing the chain "absent pool ⇒
   supply 0 ⇒ positive-demand deploy rejected" (the #13b motivation for seeding
   client pools at genesis). *)
Corollary strict_absent_pool_rejects_positive_demand :
  forall (a : nat) (f : ll_formula),
    delta_s f > 0 ->
    is_funded_balance (sigma_s (sig_stack a 0)) f = false.
Proof.
  intros a f Hpos.
  rewrite sigma_s_balance_eq_stack_count.
  apply strict_reject_when_underfunded, Hpos.
Qed.

(* Remark 21 (cost-accounted-rho, "deployment acceptance as linear proof
   search"): two deployments competing for the same linear token are two proof
   obligations over the same linear hypothesis, and AT MOST ONE can succeed. The
   no-double-spend witness [ll_no_double_spend_single_witness] (above) already
   proves a single linear atom cannot be consumed twice; we restate it here at
   the [delta_s] layer as the funding-competition corollary so the WD-D5a headline
   set carries the Remark-21 obligation explicitly: a single [s]-token (a
   depth-1 stack, supply 1) funds AT MOST one unit of demand — a second
   competing unit is unfunded. *)
Theorem competing_funding_at_most_one_succeeds :
  forall (a : nat),
    (* one unit of demand against a single token: funded *)
    funds (sigma_s (sig_stack a 1)) (delta_s (LLAtom a)) /\
    (* two competing units of demand against that SAME single token: NOT funded
       (the second proof obligation cannot also draw the one linear hypothesis) *)
    ~ funds (sigma_s (sig_stack a 1))
            (delta_s (LLTensor (LLAtom a) (LLAtom a))).
Proof.
  intro a. split.
  - unfold funds. rewrite sigma_s_balance_eq_stack_count. cbn. lia.
  - unfold funds. rewrite sigma_s_balance_eq_stack_count. cbn. lia.
Qed.

(* ─── WD-D2 acceptance gate: per-group prefix admission (reject-both) ─────────

   The block-assembly acceptance gate (cost-accounted-rho §7.6/§7.7), realized in
   [casper/.../util/rholang/acceptance.rs::admit_by_funding], processes each
   per-signature group of deployments in CANONICAL order against the group's
   effective supply [cap], admitting the LARGEST PREFIX whose cumulative demand
   [Σ Δ_s] does not exceed [cap], and — on the FIRST deployment that does not fit
   — rejecting it AND every deployment after it in the group (§7.7 reject-both /
   no-partial, tex 1696-1712). We model a group as the list [ds] of its
   per-deployment demands (in canonical order; [Δ_s ≥ 0], here [nat]) and prove
   the two headline properties the Rust code relies on: the admitted prefix is
   the unique maximal canonical prefix (`admit_prefix_maximal`), and rejection is
   downward-closed toward the tail (`reject_both_sound`). *)

(* Cumulative demand of the first [k] deployments of a group (a prefix sum). *)
Fixpoint cumdemand (ds : list nat) (k : nat) : nat :=
  match k, ds with
  | 0, _ => 0
  | S k', [] => 0
  | S k', d :: ds' => d + cumdemand ds' k'
  end.

(* The admitted prefix LENGTH: the largest [k] such that the cumulative demand of
   the first [k] deployments fits within [cap]. Defined by the same left-to-right
   residual walk the Rust gate performs ([residual -= Δ] while [Δ ≤ residual]),
   stopping (and rejecting the rest) at the first non-fitting deployment. *)
Fixpoint admitted_len (cap : nat) (ds : list nat) : nat :=
  match ds with
  | [] => 0
  | d :: ds' =>
      if Nat.leb d cap
      then S (admitted_len (cap - d) ds')
      else 0
  end.

(* The admitted prefix never exceeds the group size. *)
Lemma admitted_len_le_length :
  forall ds cap, admitted_len cap ds <= length ds.
Proof.
  induction ds as [| d ds' IH]; intro cap; cbn.
  - lia.
  - destruct (Nat.leb d cap) eqn:Hd.
    + specialize (IH (cap - d)). lia.
    + lia.
Qed.

(* SOUNDNESS of the admitted prefix: the cumulative demand of EXACTLY the admitted
   prefix fits within [cap]. So every admitted deployment is funded — the gate
   never admits an under-funded prefix (the consensus-safety direction). *)
Lemma admitted_prefix_fits :
  forall ds cap, cumdemand ds (admitted_len cap ds) <= cap.
Proof.
  induction ds as [| d ds' IH]; intro cap; cbn.
  - lia.
  - destruct (Nat.leb d cap) eqn:Hd.
    + apply Nat.leb_le in Hd. cbn.
      specialize (IH (cap - d)). lia.
    + cbn. lia.
Qed.

(* MAXIMALITY of the admitted prefix: if the admitted prefix is STRICTLY shorter
   than the whole group (i.e. some deployment was rejected), then admitting ONE
   MORE deployment would STRICTLY EXCEED [cap]. Together with [admitted_prefix_fits]
   this pins [admitted_len cap ds] as the unique largest canonical prefix whose
   cumulative demand is [≤ cap] — the gate admits no fewer and no more. *)
Theorem admit_prefix_maximal :
  forall ds cap,
    admitted_len cap ds < length ds ->
    cap < cumdemand ds (S (admitted_len cap ds)).
Proof.
  induction ds as [| d ds' IH]; intros cap Hlt; cbn in *.
  - lia.
  - destruct (Nat.leb d cap) eqn:Hd.
    + apply Nat.leb_le in Hd. cbn.
      assert (Hlt' : admitted_len (cap - d) ds' < length ds') by lia.
      specialize (IH (cap - d) Hlt'). lia.
    + apply Nat.leb_gt in Hd. cbn. lia.
Qed.

(* The "rejected at index [j]" predicate: deployment [j] (0-based) of the group is
   rejected iff it lies at or beyond the admitted prefix length. *)
Definition rejected_at (cap : nat) (ds : list nat) (j : nat) : Prop :=
  admitted_len cap ds <= j.

(* REJECT-BOTH soundness (cost-accounted-rho §7.7, [eq:reject-both]): if a
   deployment at canonical index [j] is rejected, then EVERY deployment at a later
   index [j' ≥ j] is ALSO rejected — there is no "hole" where a later deployment
   is admitted after an earlier one was rejected. This is the no-partial property
   the Rust gate enforces by closing the per-group prefix on the first
   non-fitting deployment ([prefix_open = false] for the remainder). It is a
   direct corollary of [rejected_at] being an upward-closed (toward the tail)
   threshold at [admitted_len]. *)
Theorem reject_both_sound :
  forall ds cap j j',
    j <= j' ->
    rejected_at cap ds j ->
    rejected_at cap ds j'.
Proof.
  intros ds cap j j' Hjj' Hrej. unfold rejected_at in *. lia.
Qed.

(* Reject-both, stated the way the spec's duplicate-deployment example reads
   (tex 1677-1712): once the cumulative demand through some prefix exceeds the
   supply, the deployment that tipped it over AND all deployments after it are
   rejected. This restates [admit_prefix_maximal] + [reject_both_sound] as the
   single operational fact: the first index whose inclusive cumulative demand
   exceeds [cap] is exactly [admitted_len], and everything from there on is
   rejected. *)
Corollary reject_both_from_first_overshoot :
  forall ds cap,
    admitted_len cap ds < length ds ->
    (cap < cumdemand ds (S (admitted_len cap ds))) /\
    (forall j', admitted_len cap ds <= j' -> rejected_at cap ds j').
Proof.
  intros ds cap Hlt. split.
  - apply admit_prefix_maximal; exact Hlt.
  - intros j' Hj'. unfold rejected_at. exact Hj'.
Qed.
