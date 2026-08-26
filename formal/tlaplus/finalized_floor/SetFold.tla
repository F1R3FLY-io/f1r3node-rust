----------------------------- MODULE SetFold -----------------------------

RECURSIVE FoldFiniteSet(_, _, _)

FoldFiniteSet(operator(_, _), value, set) ==
  IF set = {}
  THEN value
  ELSE LET element == CHOOSE candidate \in set : TRUE
       IN FoldFiniteSet(operator, operator(value, element), set \ {element})

=============================================================================
