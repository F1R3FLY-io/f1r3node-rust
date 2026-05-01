(*
  Names.thy --- operations on names: atoms_of, forgeability, public set.

  Defines:
    atoms_of_name :: name => atom set    (mutual with atoms_of_par)
    forgeable_by  :: name => atom set => bool
    bundle_cap_of :: name => cap         (the effective capability after
                                          collapsing nested bundles)
*)

theory Names
  imports Syntax
begin

text \<open>
  The set of \<^const>\<open>GPrivate\<close> atoms occurring anywhere inside a name or a
  process, including under quotation and under bundle wrappers.  Defined by
  mutual recursion to follow the syntactic mutual structure.
\<close>

primrec
  atoms_of_name :: "name \<Rightarrow> atom set" and
  atoms_of_par  :: "par \<Rightarrow> atom set"
where
  "atoms_of_name (GPrivate a) = {a}"
| "atoms_of_name (GDeployId _) = {}"
| "atoms_of_name (GDeployerId _) = {}"
| "atoms_of_name GSysAuthToken = {}"
| "atoms_of_name (GUri _) = {}"
| "atoms_of_name (Quote p) = atoms_of_par p"
| "atoms_of_name (Bundle _ n) = atoms_of_name n"

| "atoms_of_par Nil = {}"
| "atoms_of_par (PPar p q) = atoms_of_par p \<union> atoms_of_par q"
| "atoms_of_par (Send c ds _) = atoms_of_name c \<union> (\<Union>p \<in> set ds. atoms_of_par p)"
| "atoms_of_par (Recv binds body _ _ guard) =
     (\<Union>(ps, c) \<in> set binds. atoms_of_name c \<union> (\<Union>p \<in> set ps. atoms_of_par p))
     \<union> atoms_of_par body
     \<union> (case guard of None \<Rightarrow> {} | Some g \<Rightarrow> atoms_of_par g)"
| "atoms_of_par (NewN bound body) = atoms_of_par body - bound"
| "atoms_of_par (Match tgt cases) =
     atoms_of_par tgt
     \<union> (\<Union>(pat, gd, body) \<in> set cases.
          atoms_of_par pat
          \<union> (case gd of None \<Rightarrow> {} | Some g \<Rightarrow> atoms_of_par g)
          \<union> atoms_of_par body)"
| "atoms_of_par (IfThenElse c t e) =
     atoms_of_par c \<union> atoms_of_par t \<union> atoms_of_par e"
| "atoms_of_par (EvalQuote n) = atoms_of_name n"
| "atoms_of_par (EExpr e) =
     (\<Union>n \<in> expr_subterm_names e. atoms_of_name n)
     \<union> (\<Union>p \<in> expr_subterm_pars e. atoms_of_par p)"

text \<open>
  Forgeability of a name relative to a knowledge set \<open>K\<close>.  A name is
  forgeable if every private atom inside it is either in \<open>K\<close> or in the
  public ambient set \<^const>\<open>pub\<close>.  Otherwise the context cannot mention the
  name without revealing knowledge it does not have.
\<close>

definition forgeable_by :: "name \<Rightarrow> atom set \<Rightarrow> bool" where
  "forgeable_by n K \<longleftrightarrow> atoms_of_name n \<subseteq> K \<union> pub"

text \<open>
  Effective bundle capability of a name, collapsing nested wrappers via
  \<^const>\<open>cap_meet\<close>.  A non-bundled name behaves like \<^const>\<open>CapRW\<close>.
\<close>

primrec bundle_cap_of :: "name \<Rightarrow> cap" where
  "bundle_cap_of (GPrivate _)   = CapRW"
| "bundle_cap_of (GDeployId _)  = CapRW"
| "bundle_cap_of (GDeployerId _) = CapRW"
| "bundle_cap_of GSysAuthToken  = CapRW"
| "bundle_cap_of (GUri _)       = CapRW"
| "bundle_cap_of (Quote _)      = CapRW"
| "bundle_cap_of (Bundle c n)   = cap_meet c (bundle_cap_of n)"

text \<open>
  The underlying name with all bundle wrappers stripped.  Used to compare
  identities through bundles when reasoning about which name a sync targets.
\<close>

primrec strip_bundle :: "name \<Rightarrow> name" where
  "strip_bundle (GPrivate a)   = GPrivate a"
| "strip_bundle (GDeployId b)  = GDeployId b"
| "strip_bundle (GDeployerId b) = GDeployerId b"
| "strip_bundle GSysAuthToken  = GSysAuthToken"
| "strip_bundle (GUri u)       = GUri u"
| "strip_bundle (Quote p)      = Quote p"
| "strip_bundle (Bundle _ n)   = strip_bundle n"

end
