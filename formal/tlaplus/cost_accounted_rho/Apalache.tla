--------------------------- MODULE Apalache ---------------------------

RECURSIVE ApaFoldSet(_, _, _)

ApaFoldSet(operator(_, _), value, set) ==
    IF set = {}
    THEN value
    ELSE LET element == CHOOSE candidate \in set : TRUE
         IN ApaFoldSet(
              operator,
              operator(value, element),
              set \ {element})

=======================================================================
