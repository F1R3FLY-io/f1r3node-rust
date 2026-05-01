(*
  FreeNames.thy --- free names, bound atoms, and escape analysis.
*)

theory FreeNames
  imports Reduction
begin

text \<open>
  Free names of a process: names occurring in send-channel, receive-channel,
  payload, or guard positions.
\<close>

primrec
  free_names_par  :: "par \<Rightarrow> name set" and
  free_names_name :: "name \<Rightarrow> name set"
where
  "free_names_par Nil = {}"
| "free_names_par (PPar p q) = free_names_par p \<union> free_names_par q"
| "free_names_par (Send c d _) =
     {c} \<union> free_names_name c \<union> free_names_par d"
| "free_names_par (Recv pat c body _ _ guard) =
     free_names_par pat \<union> {c} \<union> free_names_name c
     \<union> free_names_par body \<union> free_names_par guard"
| "free_names_par (NewN _ body) = free_names_par body"
| "free_names_par (MatchOne tgt pat gd body fall) =
     free_names_par tgt \<union> free_names_par pat \<union> free_names_par gd
     \<union> free_names_par body \<union> free_names_par fall"
| "free_names_par (IfThenElse c t e) =
     free_names_par c \<union> free_names_par t \<union> free_names_par e"
| "free_names_par (EvalQuote n) = {n} \<union> free_names_name n"
| "free_names_par (EExpr ps ns) =
     \<Union> (set (map free_names_par ps)) \<union> set ns
     \<union> \<Union> (set (map free_names_name ns))"

| "free_names_name (GPrivate _)   = {}"
| "free_names_name (GDeployId _)  = {}"
| "free_names_name (GDeployerId _) = {}"
| "free_names_name GSysAuthToken  = {}"
| "free_names_name (GUri _)       = {}"
| "free_names_name (Quote p)      = free_names_par p"
| "free_names_name (Bundle _ n)   = {n} \<union> free_names_name n"

text \<open>Bound atoms introduced by all \<open>new\<close> binders in \<open>P\<close>.\<close>

primrec bn_new_par :: "par \<Rightarrow> atom set" where
  "bn_new_par Nil = {}"
| "bn_new_par (PPar p q) = bn_new_par p \<union> bn_new_par q"
| "bn_new_par (Send _ d _) = bn_new_par d"
| "bn_new_par (Recv pat _ body _ _ guard) =
     bn_new_par pat \<union> bn_new_par body \<union> bn_new_par guard"
| "bn_new_par (NewN bound body) = set bound \<union> bn_new_par body"
| "bn_new_par (MatchOne tgt pat gd body fall) =
     bn_new_par tgt \<union> bn_new_par pat \<union> bn_new_par gd
     \<union> bn_new_par body \<union> bn_new_par fall"
| "bn_new_par (IfThenElse c t e) = bn_new_par c \<union> bn_new_par t \<union> bn_new_par e"
| "bn_new_par (EvalQuote _) = {}"
| "bn_new_par (EExpr ps _) = \<Union> (set (map bn_new_par ps))"

text \<open>
  Names appearing in send-channel position.  Used by GC1's one-sided
  analysis.
\<close>

primrec sync_chans_send :: "par \<Rightarrow> name set" where
  "sync_chans_send Nil = {}"
| "sync_chans_send (PPar p q) = sync_chans_send p \<union> sync_chans_send q"
| "sync_chans_send (Send c _ _) = {c}"
| "sync_chans_send (Recv _ _ body _ _ _) = sync_chans_send body"
| "sync_chans_send (NewN _ body) = sync_chans_send body"
| "sync_chans_send (MatchOne _ _ _ body fall) =
     sync_chans_send body \<union> sync_chans_send fall"
| "sync_chans_send (IfThenElse _ t e) = sync_chans_send t \<union> sync_chans_send e"
| "sync_chans_send (EvalQuote _) = {}"
| "sync_chans_send (EExpr ps _) = \<Union> (set (map sync_chans_send ps))"

text \<open>Names appearing in receive-channel position.\<close>

primrec sync_chans_recv :: "par \<Rightarrow> name set" where
  "sync_chans_recv Nil = {}"
| "sync_chans_recv (PPar p q) = sync_chans_recv p \<union> sync_chans_recv q"
| "sync_chans_recv (Send _ _ _) = {}"
| "sync_chans_recv (Recv _ c body _ _ _) = {c} \<union> sync_chans_recv body"
| "sync_chans_recv (NewN _ body) = sync_chans_recv body"
| "sync_chans_recv (MatchOne _ _ _ body fall) =
     sync_chans_recv body \<union> sync_chans_recv fall"
| "sync_chans_recv (IfThenElse _ t e) = sync_chans_recv t \<union> sync_chans_recv e"
| "sync_chans_recv (EvalQuote _) = {}"
| "sync_chans_recv (EExpr ps _) = \<Union> (set (map sync_chans_recv ps))"

text \<open>
  Names appearing as a payload of a reachable send.  An atom escapes \<open>P\<close>
  if it appears inside any payload name.
\<close>

primrec payload_names :: "par \<Rightarrow> name set" where
  "payload_names Nil = {}"
| "payload_names (PPar p q) = payload_names p \<union> payload_names q"
| "payload_names (Send _ d _) = free_names_par d \<union> payload_names d"
| "payload_names (Recv _ _ body _ _ _) = payload_names body"
| "payload_names (NewN _ body) = payload_names body"
| "payload_names (MatchOne _ _ _ body fall) = payload_names body \<union> payload_names fall"
| "payload_names (IfThenElse _ t e) = payload_names t \<union> payload_names e"
| "payload_names (EvalQuote _) = {}"
| "payload_names (EExpr ps _) = \<Union> (set (map payload_names ps))"

text \<open>An atom escapes \<open>P\<close> if it appears in some payload-name.\<close>

definition escapes_in_par :: "par \<Rightarrow> atom \<Rightarrow> bool" where
  "escapes_in_par P a \<longleftrightarrow> (\<exists>n \<in> payload_names P. a \<in> atoms_of_name n)"

end
