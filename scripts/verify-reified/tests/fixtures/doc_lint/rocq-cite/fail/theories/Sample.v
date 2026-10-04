(* doc_lint rocq-cite fixture. The doc fixtures cite these line numbers. *)
Definition sample_value : nat := 3.

Lemma sample_value_is_three : sample_value = 3.
Proof. reflexivity. Qed.

(* Lemma hidden_name : True. This declaration is inside a comment. *)
