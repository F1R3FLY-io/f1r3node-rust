; Independent finite proof of EPathMap's neutral/set/map mode dispatch.
; A satisfiable result would be a counterexample to one of the required laws.

(set-logic ALL)

(declare-datatypes () ((Mode Neutral SetMode MapMode)))
(declare-datatypes () ((ModeResult (ModeValue (value Mode)) ModeMismatch)))

(define-fun join-mode ((left Mode) (right Mode)) ModeResult
  (ite (= left Neutral)
       (ModeValue right)
       (ite (= right Neutral)
            (ModeValue left)
            (ite (and (= left SetMode) (= right SetMode))
                 (ModeValue SetMode)
                 (ite (and (= left MapMode) (= right MapMode))
                      (ModeValue MapMode)
                      ModeMismatch)))))

(define-fun laws-hold ((mode Mode)) Bool
  (and
    (= (join-mode Neutral mode) (ModeValue mode))
    (= (join-mode mode Neutral) (ModeValue mode))
    (= (join-mode SetMode SetMode) (ModeValue SetMode))
    (= (join-mode MapMode MapMode) (ModeValue MapMode))
    (= (join-mode SetMode MapMode) ModeMismatch)
    (= (join-mode MapMode SetMode) ModeMismatch)))

(declare-const counterexample Mode)
(assert (not (laws-hold counterexample)))
(check-sat)
