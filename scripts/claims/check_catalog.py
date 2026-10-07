import argparse
import csv
import hashlib
import io
import re
import sys
from collections import Counter
from pathlib import Path, PurePosixPath

FIELDS = (
    "claim_id",
    "paper",
    "label",
    "claim_class",
    "status",
    "boundary",
    "source_refs",
    "limitation",
)
PAPERS = {
    "rho": (
        "cost-accounting/cost-accounted-rho.tex",
        "71c01eb67e2d3447c5f8e6f90e494e7120587991165466382b53b0565dc0051c",
    ),
    "monad": (
        "cost-accounting-as-monad/continued-gslt-cost-v2.tex",
        "c91a5f2f75ff960e78c415249945084583d493c31bf7b1d7b1a5088a898cc9b6",
    ),
    "knotted": (
        "knotted-topoi/knotted-topoi.tex",
        "02a5b1349e3d6a8d5157b7173e00e8d3e83fbfcae742136e20025497acbe96af",
    ),
}
REQUIRED = {
    "CA3P-TRANSFER-SUGAR": ("rho", "eq:sugar-lollipop", "operational_correspondence"),
    "CA3P-UNIFORM-SUGAR": ("rho", "eq:sugar-uniform", "operational_correspondence"),
    "CA3P-TENSOR-HOM": ("rho", "rem:tensor-hom", "adjunction"),
    "CA3P-GROUND-ENCODING": ("rho", "app:sig-trans", "injectivity"),
    "CA3P-DIGEST": ("monad", "sec:signatures", "injectivity"),
    "CA3P-RESOURCE-FORMULAS": ("monad", "sec:types", "internal_logic"),
    "CA3P-FEE-MAPPING": ("rho", "eq:fee-extract", "operational_correspondence"),
    "CA3P-ACCEPTANCE-DISCOVERY": (
        "rho", "sec:acceptance-protocol", "operational_correspondence"
    ),
    "CA3P-EXIST": ("knotted", "ob:exist", "infinity"),
    "CA3P-COHERE": ("knotted", "ob:cohere", "reflection_equivalence"),
    "CA3P-OPCORR": ("knotted", "ob:opcorr", "operational_correspondence"),
    "CA3P-CLASSIFY": ("knotted", "ob:classify", "classifier"),
    "CA3P-BARBED": ("knotted", "ob:barbed", "full_abstraction"),
    "CA3P-FUNCTOR": ("knotted", "ob:functor", "functor"),
    "CA3P-SIZE": ("knotted", "ob:size", "infinity"),
    "CA3P-METRIC": ("knotted", "ob:metric", "metric"),
    "CA3P-PLACE": ("knotted", "ob:place", "placement"),
}
BOUNDARIES = {
    "open": {"unmapped-obligation"},
    "source_inspected": {"abstract-algebra", "runtime-source"},
    "assumption": {"encoding-premise", "cryptographic-premise"},
}


class CatalogError(ValueError):
    pass


def parse_catalog(text):
    reader = csv.reader(io.StringIO(text), delimiter="\t", strict=True)
    try:
        if tuple(next(reader, ())) != FIELDS:
            raise CatalogError("The catalog header must match the documented schema.")
        rows = []
        seen = set()
        for line, fields in enumerate(reader, 2):
            if len(fields) != len(FIELDS):
                raise CatalogError(f"Line {line}: wrong field count.")
            if any(not value or value != value.strip() for value in fields):
                raise CatalogError(f"Line {line}: empty or padded field.")
            if any(
                any(ord(char) < 32 or ord(char) == 127 for char in value)
                for value in fields
            ):
                raise CatalogError(f"Line {line}: control character in field.")
            row = dict(zip(FIELDS, fields))
            key = row["claim_id"]
            if not re.fullmatch(r"CA3P-[A-Z0-9]+(?:-[A-Z0-9]+)*", key):
                raise CatalogError(f"Line {line}: invalid claim identifier.")
            if key in seen:
                raise CatalogError(f"Line {line}: duplicate claim {key}.")
            seen.add(key)
            if row["paper"] not in PAPERS:
                raise CatalogError(f"{key}: unknown paper.")
            if not re.fullmatch(r"[a-z][a-z0-9-]*:[a-z][a-z0-9-]*", row["label"]):
                raise CatalogError(f"{key}: invalid paper label.")
            if row["claim_class"] not in {item[2] for item in REQUIRED.values()}:
                raise CatalogError(f"{key}: unknown claim class.")
            if row["boundary"] not in BOUNDARIES.get(row["status"], set()):
                raise CatalogError(f"{key}: unsupported status or proof boundary.")
            if row["limitation"] == "-":
                raise CatalogError(f"{key}: an explicit limitation is required.")
            if row["status"] != "open" and row["source_refs"] == "-":
                raise CatalogError(f"{key}: source references are required.")
            if key in REQUIRED:
                actual = tuple(
                    row[field] for field in ("paper", "label", "claim_class")
                )
                if actual != REQUIRED[key]:
                    raise CatalogError(f"{key}: required obligation was relabeled.")
            rows.append(row)
    except csv.Error as error:
        raise CatalogError(f"Malformed TSV: {error}") from error
    missing = REQUIRED.keys() - seen
    if missing:
        raise CatalogError("Missing required claims: " + ", ".join(sorted(missing)))
    return rows


def source_reference(reference):
    if reference.count("#") != 1:
        raise CatalogError("A source reference must contain one file#symbol pair.")
    filename, symbol = reference.split("#")
    path = PurePosixPath(filename)
    if (
        path.is_absolute()
        or path.as_posix() != filename
        or any(part in {".", ".."} for part in path.parts)
        or not filename
        or "\\" in filename
        or ":" in filename
    ):
        raise CatalogError("A source path must be canonical and repository-relative.")
    if not re.fullmatch(r"[A-Za-z_][A-Za-z_0-9]*", symbol):
        raise CatalogError("A source symbol must be an unqualified identifier.")
    return filename, symbol


def validate_references(rows, read_source, read_paper):
    papers = {}
    sources = {}
    for row in rows:
        paper = row["paper"]
        if paper not in papers:
            filename, expected = PAPERS[paper]
            content = read_paper(filename)
            if hashlib.sha256(content).hexdigest() != expected:
                raise CatalogError(
                    f"{paper}: specification digest changed. Review the catalog."
                )
            papers[paper] = content.decode("utf-8")
        label = re.escape(row["label"])
        if not re.search(r"\\label\s*\{" + label + r"\}", papers[paper]):
            raise CatalogError(f"{row['claim_id']}: paper label is absent.")
        if row["source_refs"] == "-":
            continue
        references = row["source_refs"].split(";")
        if len(set(references)) != len(references):
            raise CatalogError(f"{row['claim_id']}: duplicate source reference.")
        for reference in references:
            filename, symbol = source_reference(reference)
            if filename not in sources:
                sources[filename] = read_source(filename).decode("utf-8")
            if not re.search(r"\b" + re.escape(symbol) + r"\b", sources[filename]):
                raise CatalogError(
                    f"{row['claim_id']}: source symbol is absent: {reference}"
                )


def read_within(root, filename):
    root = root.resolve()
    path = (root / filename).resolve()
    if not path.is_relative_to(root):
        raise CatalogError("A resolved input path escapes its input root.")
    return path.read_bytes()


def main(argv=None):
    root = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(
        description="Check the claim inventory, not theorem truth or release readiness."
    )
    parser.add_argument(
        "--catalog",
        type=Path,
        default=root / "formal/catalog/cost-accounted-rho-three-paper.tsv",
    )
    parser.add_argument(
        "--publications-root", type=Path, default=root.parent / "publications"
    )
    options = parser.parse_args(argv)
    try:
        rows = parse_catalog(options.catalog.read_text(encoding="utf-8"))
        validate_references(
            rows,
            lambda name: read_within(root, name),
            lambda name: read_within(options.publications_root, name),
        )
    except (CatalogError, OSError, UnicodeError) as error:
        print(f"Claim catalog error: {error}", file=sys.stderr)
        return 1
    counts = Counter(row["status"] for row in rows)
    summary = ", ".join(f"{status}={count}" for status, count in sorted(counts.items()))
    print(f"Claim inventory checks passed: {len(rows)} records ({summary}).")
    print("This result certifies no proof, runtime behavior, or release readiness.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
