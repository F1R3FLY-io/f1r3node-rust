(= (parent Tom) Bob)
(= (parent Tom) Liz)

(= (parent Bob) Ann)
(= (parent Liz) Pat)

(= (grandparent $x) (parent (parent $x)))

!(grandparent Tom)
