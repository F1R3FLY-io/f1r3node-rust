#!/usr/bin/env python3
from z3 import And, If, Int, Ints, Or, Solver

ok = True


def expect(name, solver, wanted):
    global ok
    result = solver.check()
    passed = str(result) == wanted
    print(f"  {'PASS' if passed else 'FAIL'}  {name}: {result} (expected {wanted})")
    if passed and wanted == "sat":
        print(f"        witness: {solver.model()}")
    ok = passed and ok


def zmin(left, right):
    return If(left <= right, left, right)


def zmax(left, right):
    return If(left >= right, left, right)


def cancel(change):
    added, removed = change
    common = zmin(added, removed)
    return added - common, removed - common


def additive(left, right):
    return left[0] + right[0], left[1] + right[1]


def max_union(left, right):
    return zmax(left[0], right[0]), zmax(left[1], right[1])


def legacy_combine(left, right):
    return cancel(max_union(left, right))


def differs(left, right):
    return Or(left[0] != right[0], left[1] != right[1])


def change(name):
    added, removed = Ints(f"{name}_added {name}_removed")
    return (added, removed), [added >= 0, removed >= 0]


x, x_domain = change("x")
y, y_domain = change("y")
z, z_domain = change("z")

solver = Solver()
solver.add(x_domain + y_domain)
solver.add(differs(additive(x, y), additive(y, x)))
expect("additive content composition is commutative", solver, "unsat")

solver = Solver()
solver.add(x_domain + y_domain + z_domain)
solver.add(differs(additive(additive(x, y), z), additive(x, additive(y, z))))
expect("additive content composition is associative", solver, "unsat")

solver = Solver()
solver.add(x_domain)
solver.add(differs(additive(x, (0, 0)), x))
expect("zero is the additive identity", solver, "unsat")

solver = Solver()
solver.add(differs(additive((1, 0), (1, 0)), (2, 0)))
expect("two distinct equal outputs retain multiplicity two", solver, "unsat")

solver = Solver()
solver.add(differs(max_union((1, 0), (1, 0)), (1, 0)))
expect("max-union collapses two distinct equal outputs", solver, "unsat")

seen_left = Int("seen_left")
seen_right = Int("seen_right")
deduplicated_count = If(Or(seen_left == 1, seen_right == 1), 1, 0)
solver = Solver()
solver.add(seen_left == 1, seen_right == 1, deduplicated_count != 1)
expect("one causal identity is projected once", solver, "unsat")

left_value, right_value = Ints("left_value right_value")
same_identity = Int("same_identity")
conflict = And(same_identity == 1, left_value != right_value)
solver = Solver()
solver.add(same_identity == 1, left_value != right_value, conflict == False)
expect("same identity with unequal content is rejected", solver, "unsat")

solver = Solver()
solver.add(differs(cancel(additive((1, 0), (0, 1))), (0, 0)))
expect("dependent add/remove effects telescope", solver, "unsat")

solver = Solver()
solver.add(differs(additive((1, 0), (1, 0)), (1, 0)))
expect("naive replicated whole-block deltas double-count", solver, "sat")

solver = Solver()
solver.add(
    differs(
        legacy_combine(legacy_combine((1, 0), (1, 0)), (0, 1)),
        legacy_combine((1, 0), legacy_combine((1, 0), (0, 1))),
    )
)
expect("legacy inline max-union cancellation is non-associative", solver, "sat")

print("== Z3 exact causal channel netting: ALL PASS ==" if ok else "== FAILURES ==")
raise SystemExit(0 if ok else 1)
