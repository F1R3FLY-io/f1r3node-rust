#!/usr/bin/env python3
"""The pure-Python receipt of the VectorDB numeric core (plan decision 13, P6 step 2).

The receipt recomputes every case of test-vectors/numeric-reference-v1.json with the
Python standard library only, and fails on any difference. The Sage leg
(formal/sage/reified_rspaces/similarity_metrics.sage) generates the file, and the Rust
test tensor_numeric_reference_vectors_agree (tensor/exact.rs) reads it. So three
independent implementations agree on every value.

The receipt uses other methods than the Sage leg and the Rust code:

  * Exact values are fractions.Fraction. A float element enters through
    Fraction(float), which is exact.
  * RN_53 of a rational is float(Fraction): CPython divides the numerator by the
    denominator with correct rounding, ties to even, subnormals included, and it
    raises OverflowError beyond the finite range (ValueOutOfRange).
  * RN_53 of a square root comes from a 600-digit decimal square root and the
    correctly rounded conversion of its decimal string to a float. The 600 digits
    put the approximation far below half a unit in the last place of any value of
    the file, so the second rounding is the rounding of the root.
  * The cosine and euclidean bounds are decided by the squared forms over Fraction.
  * A zero result is +0, as the Rust contract states.

Exit status: 0 when every case agrees, 1 otherwise, 2 on a usage or input error.
Python 3 standard library only.
"""

from __future__ import annotations

import argparse
import decimal
import json
import struct
import sys
from fractions import Fraction
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_VECTORS = REPO_ROOT / "test-vectors" / "numeric-reference-v1.json"
SCHEMA = "reified-rspaces-numeric-reference/1"
ZERO_BITS = "0x0000000000000000"
OUT_OF_RANGE = "ValueOutOfRange"


def bits_of(value: float) -> str:
    if value == 0.0:
        return ZERO_BITS
    return "0x%016x" % struct.unpack(">Q", struct.pack(">d", value))[0]


def element(text: str) -> Fraction:
    """The exact value of an element: "n", "p/q", "f64:<hex bits>" or "bool:<b>"."""
    if text.startswith("f64:"):
        return Fraction(struct.unpack(">d", bytes.fromhex(text[4:]))[0])
    if text.startswith("bool:"):
        return Fraction(1 if text[5:] == "true" else 0)
    return Fraction(text)


def rn53(x: Fraction) -> str:
    try:
        return bits_of(float(x))
    except OverflowError:
        return OUT_OF_RANGE


def rn53_sqrt(x: Fraction, sign: int = 1) -> str:
    """RN_53 of sign * sqrt(x) for x >= 0, by a 600-digit decimal square root."""
    if x == 0:
        return ZERO_BITS
    context = decimal.Context(prec=600, Emax=decimal.MAX_EMAX, Emin=decimal.MIN_EMIN)
    root = context.sqrt(context.divide(decimal.Decimal(x.numerator), decimal.Decimal(x.denominator)))
    value = float(str(root))
    if value == float("inf"):
        return OUT_OF_RANGE
    return bits_of(sign * value)


def dot(a: list[Fraction], b: list[Fraction]) -> Fraction:
    return sum((x * y for x, y in zip(a, b)), Fraction(0))


def is_binary(v: list[Fraction]) -> bool:
    return all(x in (0, 1) for x in v)


def expected(case: dict) -> dict:
    metric = case["metric"]
    q = [element(x) for x in case["query"]]
    e = [element(x) for x in case["record"]]
    bound = case["bound"]
    threshold = Fraction(bound["threshold"]) if bound and "threshold" in bound else None
    radius = Fraction(bound["radius"]) if bound and "radius" in bound else None
    out = {"query_refused": False, "score_defined": True, "delivered": None, "within": None}
    if metric == "cosine":
        nq, ne = dot(q, q), dot(e, e)
        if nq == 0:
            out.update(query_refused=True, score_defined=False)
            return out
        if ne == 0:
            out.update(score_defined=False)
            return out
        u, n = dot(q, e), nq * ne
        out["delivered"] = rn53_sqrt(u * u / n, -1 if u < 0 else 1)
        if threshold is not None:
            t = threshold
            out["within"] = ((u >= 0 and t < 0)
                             or (u >= 0 and t >= 0 and t * t * n <= u * u)
                             or (u < 0 and t < 0 and u * u <= t * t * n))
        return out
    if metric == "dot":
        value = dot(q, e)
        out["delivered"] = rn53(value)
        if threshold is not None:
            out["within"] = value >= threshold
        return out
    if metric == "euclidean":
        d2 = sum(((x - y) * (x - y) for x, y in zip(q, e)), Fraction(0))
        out["delivered"] = rn53_sqrt(d2)
        if radius is not None:
            out["within"] = radius >= 0 and d2 <= radius * radius
        return out
    if metric == "manhattan":
        value = sum((abs(x - y) for x, y in zip(q, e)), Fraction(0))
        out["delivered"] = rn53(value)
        if radius is not None:
            out["within"] = value <= radius
        return out
    if not (is_binary(q) and is_binary(e)):
        out.update(score_defined=False)
        return out
    if metric == "hamming":
        count = sum(1 for x, y in zip(q, e) if x != y)
        out["delivered"] = rn53(Fraction(count))
        if radius is not None:
            out["within"] = count <= radius
        return out
    if metric == "jaccard":
        inter = sum(1 for x, y in zip(q, e) if x == 1 and y == 1)
        union = sum(1 for x, y in zip(q, e) if x == 1 or y == 1)
        if union == 0:
            out.update(score_defined=False)
            return out
        out["delivered"] = rn53(Fraction(inter, union))
        if threshold is not None:
            out["within"] = inter >= threshold * union
        return out
    raise ValueError(f"unknown metric {metric}")


def main() -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").split("\n\n")[0])
    parser.add_argument("--vectors", type=Path, default=DEFAULT_VECTORS)
    args = parser.parse_args()
    try:
        document = json.loads(args.vectors.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        print(f"error: cannot read {args.vectors}: {error}", file=sys.stderr)
        return 2
    if document.get("schema") != SCHEMA:
        print(f"error: {args.vectors} has schema {document.get('schema')}, expected {SCHEMA}",
              file=sys.stderr)
        return 2
    passed = failed = 0

    def check(name: str, got, want) -> None:
        nonlocal passed, failed
        if got == want:
            passed += 1
        else:
            failed += 1
            print(f"  FAIL {name}: receipt {got}, file {want}")

    for case in document["rounding_cases"]:
        check(f"rounding {case['x'][:40]}", rn53(Fraction(case["x"])), case["rn53"])
    for case in document["sqrt_cases"]:
        check(f"square root {case['x'][:40]}", rn53_sqrt(Fraction(case["x"])), case["rn53"])
    for case in document["metric_cases"]:
        want = {key: case[key] for key in ("query_refused", "score_defined", "delivered", "within")}
        check(f"metric {case['name']}", expected(case), want)
    print(f"numeric reference receipt: PASS={passed} FAIL={failed} ({args.vectors})")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
