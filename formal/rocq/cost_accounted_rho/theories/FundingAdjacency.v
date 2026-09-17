From Stdlib Require Import Arith.PeanoNat Bool.Bool Lists.List Lia.
From CostAccountedRho Require Import FundingParentPath.
Import ListNotations.

Definition adjacency_parent (next : nat -> option nat) vertex :=
  match vertex with
  | 0 => 0
  | S index => match next index with None => 0 | Some previous => S previous end
  end.

Definition adjacency_ranked count (next : nat -> option nat) :=
  forall index previous, index < count -> next index = Some previous -> previous < index.

Theorem adjacency_links_establish_ranked_parents : forall count next,
  adjacency_ranked count next ->
  forall vertex, (vertex <=? count) = true -> vertex <> 0 ->
    (adjacency_parent next vertex <=? count) = true /\
    adjacency_parent next vertex < vertex /\
    Nat.eqb (adjacency_parent next vertex) (adjacency_parent next vertex) = true.
Proof.
  intros count next ranked vertex inside nonzero.
  apply Nat.leb_le in inside. destruct vertex as [|index]; [contradiction|].
  unfold adjacency_parent. destruct (next index) as [previous|] eqn:linked.
  - specialize (ranked index previous ltac:(lia) linked).
    split; [apply Nat.leb_le; lia|]. split; [lia|apply Nat.eqb_refl].
  - split; [apply Nat.leb_le; lia|]. split; [lia|apply Nat.eqb_refl].
Qed.

Theorem ranked_adjacency_scan_terminates : forall count next start,
  adjacency_ranked count next -> start < count ->
  exists path,
    extract_ranked_parent_path count 0 (adjacency_parent next) (S start) = Some path /\
    ranked_parent_path_certificate 0 (fun vertex => vertex <=? count) (fun vertex => vertex)
      (fun from to => Nat.eqb from (adjacency_parent next to)) (S start) path.
Proof.
  intros count next start ranked inside.
  eapply strictly_ranked_parents_extract_a_simple_path.
  - exact (adjacency_links_establish_ranked_parents count next ranked).
  - apply Nat.leb_le. lia.
  - lia.
Qed.

Theorem ranked_adjacency_scan_is_bounded_and_acyclic : forall count next start,
  adjacency_ranked count next -> start < count ->
  exists path,
    extract_ranked_parent_path count 0 (adjacency_parent next) (S start) = Some path /\
    NoDup path /\ length path <= start + 2 /\ last path 0 = 0 /\
    Forall (fun vertex => vertex <= count) path.
Proof.
  intros count next start ranked inside.
  destruct (ranked_adjacency_scan_terminates count next start ranked inside) as [path [found certificate]].
  destruct certificate as [begins ending simple known links ranks bounded].
  exists path. split; [exact found|]. split; [exact simple|]. split; [lia|]. split; [exact ending|].
  rewrite Forall_forall in *. intros vertex member. specialize (known vertex member).
  now apply Nat.leb_le in known.
Qed.

Print Assumptions adjacency_links_establish_ranked_parents.
Print Assumptions ranked_adjacency_scan_terminates.
Print Assumptions ranked_adjacency_scan_is_bounded_and_acyclic.
