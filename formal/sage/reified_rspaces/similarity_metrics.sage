# Sage leg of the VectorDB numeric core (P6, design v3 sections 4.1 and 5).
#
# This script is the independent reference for rholang/src/rust/interpreter/tensor/.
# It does three things:
#
#   1. It checks the metric laws on random exact vectors (QQ): symmetry, the triangle
#      inequality of manhattan and hamming, Cauchy-Schwarz, the polarization identity
#      of euclidean, and inter <= union for jaccard.
#   2. It decides every cosine threshold and every euclidean radius with real
#      arithmetic: the cosine and the distance are exact real algebraic numbers (AA),
#      so the decision does not use the squared forms of the Rust code. Every other
#      bound is a comparison of exact rationals.
#   3. It computes every delivered value as RN_53 (the nearest IEEE-754 binary64
#      value, ties to even) of the exact value: MPFR rounds the normal range, and the
#      subnormal range rounds to multiples of 2^-1074. A value beyond the finite
#      range is ValueOutOfRange, and a zero result is +0 (the Rust contract).
#
# The output is test-vectors/numeric-reference-v1.json. The Rust test
# tensor_numeric_reference_vectors_agree (tensor/exact.rs) and the pure-Python receipt
# scripts/verify-reified/numeric_reference_receipt.py read it. The generation is
# deterministic (a fixed seed), so a run compares its result with the committed file
# and fails on any difference. Run with RRS_SAGE_WRITE=1 to regenerate the file.
#
# Usage: RRS_REPO_ROOT=<repository> [RRS_SAGE_WRITE=1] sage similarity_metrics.sage
#
# The sage launcher takes no options after the script, so the repository root and the
# write mode come from the environment. `sage` writes a preparsed copy
# (similarity_metrics.sage.py) next to the script, so a gate runs a copy of the script
# in a scratch directory under target/, never in the source tree.

import json
import os
import struct
from pathlib import Path

ROOT = Path(os.environ.get("RRS_REPO_ROOT", ".")).resolve()
OUTPUT = ROOT / "test-vectors" / "numeric-reference-v1.json"
SEED = 20261008
WRITE = os.environ.get("RRS_SAGE_WRITE") == "1"

F64_MAX = (2**53 - 1) * 2**971
MIN_NORMAL = QQ(2)**-1022
QUANTUM = QQ(2)**-1074
HIGH = RealField(4096)
R53 = RealField(53, rnd='RNDN')
ZERO_BITS = "0x0000000000000000"


# ── Exact values of the elements ───────────────────────────────────────────────

def f64_of_bits(text):
    return struct.unpack(">d", bytes.fromhex(text))[0]


def exact_of_float(value):
    numerator, denominator = float(value).as_integer_ratio()
    return QQ(numerator) / QQ(denominator)


def element_value(text):
    """The exact value of a JSON element: "p/q", "n", "f64:<hex bits>" or "bool:<b>"."""
    if text.startswith("f64:"):
        return exact_of_float(f64_of_bits(text[4:]))
    if text.startswith("bool:"):
        return QQ(1) if text[5:] == "true" else QQ(0)
    return QQ(text)


def bits_of_f64(value):
    return "0x%016x" % struct.unpack(">Q", struct.pack(">d", value))[0]


def f64_element(value):
    return "f64:%016x" % struct.unpack(">Q", struct.pack(">d", float(value)))[0]


def rational_text(value):
    value = QQ(value)
    return str(value.numerator()) if value.denominator() == 1 else "%s/%s" % (
        value.numerator(), value.denominator())


# ── RN_53 ─────────────────────────────────────────────────────────────────────

def round_half_even(y):
    """y >= 0 rounded to an integer, ties to even (y exact in QQ or a high real)."""
    n = Integer(floor(y))
    f = y - n
    if f > 1/2:
        return n + 1
    if f < 1/2:
        return n
    return n + (n % 2)


def encode(sign, magnitude_bits):
    return "0x%016x" % ((sign << 63) | magnitude_bits)


def rn53_rational(x):
    """RN_53 of an exact rational: hex bits, or ValueOutOfRange."""
    x = QQ(x)
    if x == 0:
        return ZERO_BITS
    sign = 1 if x < 0 else 0
    m = abs(x)
    if m < MIN_NORMAL:
        k = round_half_even(m / QUANTUM)
        if k == 0:
            return ZERO_BITS
        return encode(sign, int(k))  # k = 2^52 is the least normal number, bits 1 << 52
    r = R53(m)
    if r.exact_rational() > F64_MAX:
        return "ValueOutOfRange"
    return encode(sign, struct.unpack(">Q", struct.pack(">d", float(r)))[0])


def rn53_real(v):
    """RN_53 of a real algebraic number, through a 4096-bit approximation.

    A 4096-bit value differs from v by less than 2^-4000 relative. For the small
    inputs of this file v is never that close to a midpoint of two binary64 values
    unless it is that midpoint, a dyadic number that the approximation holds exactly,
    so the second rounding is the rounding of v."""
    if v == 0:
        return ZERO_BITS
    sign = 1 if v < 0 else 0
    m = HIGH(abs(v))
    if m < HIGH(MIN_NORMAL):
        k = round_half_even(m / HIGH(QUANTUM))
        if k == 0:
            return ZERO_BITS
        return encode(sign, int(k))
    r = R53(m)
    if r.exact_rational() > F64_MAX:
        return "ValueOutOfRange"
    return encode(sign, struct.unpack(">Q", struct.pack(">d", float(r)))[0])


def rn53_sqrt(x):
    return rn53_real(AA(QQ(x)).sqrt())


# ── The metrics ────────────────────────────────────────────────────────────────

METRICS = ["cosine", "dot", "euclidean", "manhattan", "hamming", "jaccard"]


def dot(a, b):
    return sum(x * y for x, y in zip(a, b))


def is_binary(v):
    return all(x in (0, 1) for x in v)


def evaluate(metric, q, e, bound):
    """The expected outcome of one query and one record (design v3, section 4.4)."""
    out = {"query_refused": False, "score_defined": True, "delivered": None, "within": None}
    if metric == "cosine":
        nq, ne = dot(q, q), dot(e, e)
        if nq == 0:
            out.update(query_refused=True, score_defined=False)
            return out
        if ne == 0:
            out.update(score_defined=False)
            return out
        cos = AA(dot(q, e)) / (AA(nq).sqrt() * AA(ne).sqrt())
        out["delivered"] = rn53_real(cos)
        if bound is not None:
            out["within"] = bool(cos >= AA(bound[1]))
        return out
    if metric == "dot":
        value = dot(q, e)
        out["delivered"] = rn53_rational(value)
        if bound is not None:
            out["within"] = bool(value >= bound[1])
        return out
    if metric == "euclidean":
        distance = AA(sum((x - y)**2 for x, y in zip(q, e))).sqrt()
        out["delivered"] = rn53_real(distance)
        if bound is not None:
            out["within"] = bool(distance <= AA(bound[1]))
        return out
    if metric == "manhattan":
        value = sum(abs(x - y) for x, y in zip(q, e))
        out["delivered"] = rn53_rational(value)
        if bound is not None:
            out["within"] = bool(value <= bound[1])
        return out
    if not (is_binary(q) and is_binary(e)):
        out.update(score_defined=False)
        return out
    if metric == "hamming":
        count = sum(1 for x, y in zip(q, e) if x != y)
        out["delivered"] = rn53_rational(count)
        if bound is not None:
            out["within"] = bool(count <= bound[1])
        return out
    inter = sum(1 for x, y in zip(q, e) if x == 1 and y == 1)
    union = sum(1 for x, y in zip(q, e) if x == 1 or y == 1)
    if union == 0:
        out.update(score_defined=False)
        return out
    out["delivered"] = rn53_rational(QQ(inter) / union)
    if bound is not None:
        out["within"] = bool(inter >= bound[1] * union)
    return out


def case(name, metric, query, record, bound=None):
    q = [element_value(x) for x in query]
    e = [element_value(x) for x in record]
    parsed = None if bound is None else (bound[0], QQ(bound[1]))
    entry = {"name": name, "metric": metric, "query": query, "record": record,
             "bound": None if bound is None else {bound[0]: bound[1]}}
    entry.update(evaluate(metric, q, e, parsed))
    return entry


# ── 1. The metric laws on random exact vectors ────────────────────────────────

set_random_seed(SEED)


def random_rational():
    return QQ(randint(-20, 20)) / QQ(randint(1, 9))


def random_vector(dimension):
    return [random_rational() for _ in range(dimension)]


checks = 0
for _ in range(200):
    dimension = randint(1, 6)
    a, b, c = (random_vector(dimension) for _ in range(3))
    assert dot(a, b) == dot(b, a), "dot is not symmetric"
    assert dot(a, b)**2 <= dot(a, a) * dot(b, b), "Cauchy-Schwarz fails"
    d2 = sum((x - y)**2 for x, y in zip(a, b))
    assert d2 == dot(a, a) - 2 * dot(a, b) + dot(b, b), "the polarization identity fails"
    assert d2 >= 0 and (d2 == 0) == (a == b), "dist2 is not a squared metric"
    l1 = lambda x, y: sum(abs(s - t) for s, t in zip(x, y))
    assert l1(a, b) == l1(b, a) and l1(a, c) <= l1(a, b) + l1(b, c), "manhattan is not a metric"
    bits = [[randint(0, 1) for _ in range(dimension)] for _ in range(3)]
    ham = lambda x, y: sum(1 for s, t in zip(x, y) if s != t)
    assert ham(bits[0], bits[2]) <= ham(bits[0], bits[1]) + ham(bits[1], bits[2]), \
        "hamming breaks the triangle inequality"
    inter = sum(1 for s, t in zip(bits[0], bits[1]) if s and t)
    union = sum(1 for s, t in zip(bits[0], bits[1]) if s or t)
    assert inter <= union, "jaccard: inter > union"
    checks += 1
print("metric laws: %d random cases pass" % checks)

# ── 2 and 3. The reference vectors ────────────────────────────────────────────

rounding_cases = []
for x in ["1/3", "2/3", "-2/3", "1/10", "-1/10", "24/25", "1/7",
          str(2**53 + 1), str(2**53 + 3), str(-(2**53 + 1)), str(2**53 - 1),
          "1/%d" % 2**1075, "3/%d" % 2**1075, "-3/%d" % 2**1075, "1/%d" % 2**1076,
          "%d/%d" % (2**52 - 1, 2**1074), "%d/%d" % (2**53 - 1, 2**1075),
          str(F64_MAX), str(F64_MAX + 2**969), str(F64_MAX + 2**970), str(2**1024),
          "-%d" % 2**1100, "5/6", "123456789/1000"]:
    rounding_cases.append({"x": x, "rn53": rn53_rational(QQ(x))})

sqrt_cases = []
for x in ["2", "1/2", "9/4", "25/36", "1/3", "10", "7/11", "3",
          str(2**2046), str(2**2048), str(2**2048 - 2**1100),
          "1/%d" % 2**2148, "1/%d" % 2**2150, "1/%d" % 2**2100, "58/441"]:
    sqrt_cases.append({"x": x, "rn53": rn53_sqrt(QQ(x))})

f = f64_element
metric_cases = [
    case("cosine-3-4-exact", "cosine", ["3", "4"], ["4", "3"], ("threshold", "24/25")),
    case("cosine-3-4-above", "cosine", ["3", "4"], ["4", "3"], ("threshold", "2401/2500")),
    case("cosine-opposite", "cosine", ["3", "4"], ["-4", "-3"], ("threshold", "0")),
    case("cosine-diagonal", "cosine", ["1", "0"], ["1", "1"], ("threshold", "7/10")),
    case("cosine-parallel-one", "cosine", ["2", "4"], ["1", "2"], ("threshold", "1")),
    case("cosine-zero-query", "cosine", ["0", "0"], ["1", "1"], ("threshold", "1/2")),
    case("cosine-zero-record", "cosine", ["1", "1"], ["0", "0"], ("threshold", "1/2")),
    case("cosine-floats", "cosine", [f(0.1), f(0.2)], [f(0.3), f(-0.1)], ("threshold", "1/10")),
    case("dot-cancellation", "dot", [f(1e16), "1", f(-1e16)], ["1", "1", "1"], ("threshold", "1/2")),
    case("dot-overflow", "dot", [f(1e308)], ["10"], ("threshold", "0")),
    case("dot-rational", "dot", ["1/3", "1/7"], ["3", "7"], ("threshold", "2")),
    case("euclidean-root-two", "euclidean", ["0", "0"], ["1", "1"], ("radius", "141421/100000")),
    case("euclidean-five", "euclidean", ["0", "0"], ["3", "4"], ("radius", "5")),
    case("euclidean-fractions", "euclidean", ["1/3", "1/7"], ["0", "0"], ("radius", "4/11")),
    case("euclidean-negative-radius", "euclidean", ["1"], ["1"], ("radius", "-1")),
    case("manhattan-floats", "manhattan", [f(0.1), f(0.2)], ["0", "0"], ("radius", "3/10")),
    case("manhattan-seven", "manhattan", ["0", "0"], ["3", "-4"], ("radius", "7")),
    case("hamming-two", "hamming", ["1", "1", "0", "0"], ["1", "0", "1", "0"], ("radius", "2")),
    case("hamming-booleans", "hamming", ["bool:true", "bool:false"], [f(1.0), f(1.0)], ("radius", "1/2")),
    case("hamming-not-binary", "hamming", ["1", "0"], ["2", "0"], ("radius", "5")),
    case("jaccard-third", "jaccard", ["1", "1", "0"], ["1", "0", "1"], ("threshold", "1/3")),
    case("jaccard-empty", "jaccard", ["0", "0"], ["0", "0"], ("threshold", "0")),
    case("jaccard-not-binary", "jaccard", ["1", "1/2"], ["1", "0"], ("threshold", "0")),
]

set_random_seed(SEED + 1)


def random_element(binary):
    if binary:
        return str(randint(0, 1))
    kind = randint(0, 2)
    if kind == 0:
        return str(randint(-20, 20))
    if kind == 1:
        return rational_text(QQ(randint(-20, 20)) / QQ(randint(1, 9)))
    return f64_element(randint(-1000, 1000) * 2.0**randint(-12, 4))


for index in range(120):
    metric = METRICS[index % len(METRICS)]
    binary = metric in ("hamming", "jaccard")
    dimension = randint(1, 5)
    query = [random_element(binary) for _ in range(dimension)]
    record = [random_element(binary) for _ in range(dimension)]
    if metric in ("cosine", "jaccard"):
        bound = ("threshold", rational_text(QQ(randint(0, 20)) / 20))
    elif metric == "dot":
        bound = ("threshold", rational_text(QQ(randint(-40, 40)) / QQ(randint(1, 4))))
    else:
        bound = ("radius", rational_text(QQ(randint(-2, 60)) / QQ(randint(1, 4))))
    metric_cases.append(case("random-%03d" % index, metric, query, record, bound))

document = {
    "schema": "reified-rspaces-numeric-reference/1",
    "generator": "formal/sage/reified_rspaces/similarity_metrics.sage",
    "description": ("Reference values of the VectorDB numeric core: RN_53 of exact "
                    "rationals and of square roots, and the outcome of one query and one "
                    "record for each metric. Elements are integers, rationals p/q, "
                    "f64:<bits> or bool:<b>. A delivered value is the hex bits of a "
                    "binary64 value (+0 for zero), ValueOutOfRange, or null when the "
                    "score is undefined. within is decided with real algebraic numbers "
                    "for cosine and euclidean."),
    "seed": int(SEED),
    "rounding_cases": rounding_cases,
    "sqrt_cases": sqrt_cases,
    "metric_cases": metric_cases,
}
text = json.dumps(document, indent=1, sort_keys=True) + "\n"

if WRITE:
    OUTPUT.write_text(text)
    print("wrote %s: %d rounding, %d square root and %d metric cases" % (
        OUTPUT, len(rounding_cases), len(sqrt_cases), len(metric_cases)))
else:
    committed = OUTPUT.read_text() if OUTPUT.exists() else None
    assert committed == text, "the reference vectors differ from %s (RRS_SAGE_WRITE=1)" % OUTPUT
    print("reference vectors: %d rounding, %d square root and %d metric cases match %s" % (
        len(rounding_cases), len(sqrt_cases), len(metric_cases), OUTPUT))
print("== Sage similarity leg: ALL PASS")
