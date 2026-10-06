import contextlib
import copy
import csv
import hashlib
import io
import itertools
import os
import random
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import check_catalog as catalog


class CatalogTests(unittest.TestCase):
    def setUp(self):
        root = Path(__file__).resolve().parents[2]
        self.text = (
            root / "formal/catalog/cost-accounted-rho-three-paper.tsv"
        ).read_text()
        self.rows = catalog.parse_catalog(self.text)
        self.paper_data = {}
        self.papers = {}
        for key, (filename, _) in catalog.PAPERS.items():
            labels = sorted({row["label"] for row in self.rows if row["paper"] == key})
            data = "\n".join("\\label{" + label + "}" for label in labels).encode()
            self.paper_data[filename] = data
            self.papers[key] = (filename, hashlib.sha256(data).hexdigest())
        self.source_data = {}
        for row in self.rows:
            if row["source_refs"] != "-":
                for reference in row["source_refs"].split(";"):
                    filename, symbol = catalog.source_reference(reference)
                    self.source_data[filename] = (
                        self.source_data.get(filename, b"") + symbol.encode() + b"\n"
                    )

    def encode(self, rows):
        output = io.StringIO()
        writer = csv.DictWriter(
            output, fieldnames=catalog.FIELDS, delimiter="\t", lineterminator="\n"
        )
        writer.writeheader()
        writer.writerows(rows)
        return output.getvalue()

    def validate(self, rows=None):
        with patch.dict(catalog.PAPERS, self.papers, clear=True):
            catalog.validate_references(
                self.rows if rows is None else rows,
                self.source_data.__getitem__,
                self.paper_data.__getitem__,
            )

    def test_catalog_covers_all_required_obligations(self):
        self.assertEqual(set(catalog.REQUIRED), {row["claim_id"] for row in self.rows})
        self.validate()

    def test_round_trip_preserves_every_field(self):
        self.assertEqual(catalog.parse_catalog(self.encode(self.rows)), self.rows)

    def test_every_required_row_is_mandatory(self):
        for omitted in self.rows:
            with (
                self.subTest(claim=omitted["claim_id"]),
                self.assertRaises(catalog.CatalogError),
            ):
                catalog.parse_catalog(
                    self.encode([row for row in self.rows if row != omitted])
                )

    def test_every_duplicate_row_is_rejected(self):
        for row in self.rows:
            with (
                self.subTest(claim=row["claim_id"]),
                self.assertRaises(catalog.CatalogError),
            ):
                catalog.parse_catalog(self.encode(self.rows + [row]))

    def test_empty_and_wrong_headers_are_rejected(self):
        for text in (
            "",
            "\n",
            "\ufeff" + self.text,
            self.text.replace("claim_id", "id", 1),
        ):
            with self.subTest(text=text[:30]), self.assertRaises(catalog.CatalogError):
                catalog.parse_catalog(text)

    def test_ragged_and_blank_rows_are_rejected(self):
        lines = self.text.splitlines()
        for row in ("", "\t".join(lines[1].split("\t")[:-1]), lines[1] + "\textra"):
            with self.subTest(row=row[:30]), self.assertRaises(catalog.CatalogError):
                catalog.parse_catalog("\n".join([lines[0], row] + lines[2:]) + "\n")

    def test_all_field_omissions_and_padding_are_rejected(self):
        for field, value in itertools.product(
            catalog.FIELDS, ("", " padded", "padded ")
        ):
            rows = copy.deepcopy(self.rows)
            rows[0][field] = value
            with (
                self.subTest(field=field, value=value),
                self.assertRaises(catalog.CatalogError),
            ):
                catalog.parse_catalog(self.encode(rows))

    def test_control_characters_are_rejected(self):
        for value in [chr(i) for i in range(32)] + [chr(127)]:
            rows = copy.deepcopy(self.rows)
            rows[0]["limitation"] = "Before" + value + "after"
            with self.subTest(code=ord(value)), self.assertRaises(catalog.CatalogError):
                catalog.parse_catalog(self.encode(rows))

    def test_completion_statuses_cannot_be_claimed(self):
        for status in (
            "verified",
            "complete",
            "proved",
            "implemented",
            "excluded",
            "SOURCE_INSPECTED",
        ):
            for index, original in enumerate(self.rows):
                rows = copy.deepcopy(self.rows)
                rows[index]["status"] = status
                with (
                    self.subTest(status=status, claim=original["claim_id"]),
                    self.assertRaises(catalog.CatalogError),
                ):
                    catalog.parse_catalog(self.encode(rows))

    def test_status_boundary_combinations_are_checked(self):
        boundaries = set().union(*catalog.BOUNDARIES.values())
        for status, boundary in itertools.product(catalog.BOUNDARIES, boundaries):
            rows = copy.deepcopy(self.rows)
            rows[0].update(status=status, boundary=boundary)
            with self.subTest(status=status, boundary=boundary):
                if boundary in catalog.BOUNDARIES[status]:
                    catalog.parse_catalog(self.encode(rows))
                else:
                    with self.assertRaises(catalog.CatalogError):
                        catalog.parse_catalog(self.encode(rows))

    def test_required_claim_identity_cannot_be_reassigned(self):
        for field, value in (
            ("paper", "knotted"),
            ("label", "ob:exist"),
            ("claim_class", "classifier"),
        ):
            rows = copy.deepcopy(self.rows)
            rows[0][field] = value
            with self.subTest(field=field), self.assertRaises(catalog.CatalogError):
                catalog.parse_catalog(self.encode(rows))

    def test_non_open_rows_need_references_and_limitations(self):
        for index, row in enumerate(self.rows):
            fields = ["limitation"] + (
                ["source_refs"] if row["status"] != "open" else []
            )
            for field in fields:
                rows = copy.deepcopy(self.rows)
                rows[index][field] = "-"
                with (
                    self.subTest(claim=row["claim_id"], field=field),
                    self.assertRaises(catalog.CatalogError),
                ):
                    catalog.parse_catalog(self.encode(rows))

    def test_source_references_reject_noncanonical_paths_and_symbols(self):
        references = (
            "../outside#symbol",
            "/outside#symbol",
            "a/../b#symbol",
            "a/./b#symbol",
            "a//b#symbol",
            "./a#symbol",
            "a/#symbol",
            "#symbol",
            "a\\b#symbol",
            "C:/a#symbol",
            "a#",
            "a#b#c",
            "a",
            "a#not a symbol",
            "a#.*",
        )
        for reference in references:
            with (
                self.subTest(reference=reference),
                self.assertRaises(catalog.CatalogError),
            ):
                catalog.source_reference(reference)

    def test_missing_or_partial_symbols_are_rejected(self):
        reference = next(
            row["source_refs"]
            for row in self.rows
            if row["source_refs"] != "-" and ";" not in row["source_refs"]
        )
        filename, symbol = catalog.source_reference(reference)
        for content in (
            b"",
            (symbol + "_different").encode(),
            ("prefix_" + symbol).encode(),
        ):
            self.source_data[filename] = content
            with self.subTest(content=content), self.assertRaises(catalog.CatalogError):
                self.validate()

    def test_duplicate_references_are_rejected(self):
        rows = copy.deepcopy(self.rows)
        rows[0]["source_refs"] += ";" + rows[0]["source_refs"]
        with self.assertRaises(catalog.CatalogError):
            self.validate(rows)

    def test_changed_paper_digests_are_rejected(self):
        for filename in self.paper_data:
            original = self.paper_data[filename]
            self.paper_data[filename] = original + b"changed"
            with (
                self.subTest(filename=filename),
                self.assertRaises(catalog.CatalogError),
            ):
                self.validate()
            self.paper_data[filename] = original

    def test_missing_label_rejected_even_with_matching_digest(self):
        filename, _ = self.papers["rho"]
        content = b"No labels here"
        self.paper_data[filename] = content
        self.papers["rho"] = (filename, hashlib.sha256(content).hexdigest())
        with self.assertRaises(catalog.CatalogError):
            self.validate()

    def test_generated_reordering_preserves_results(self):
        generator = random.Random(216)
        for _ in range(200):
            rows = copy.deepcopy(self.rows)
            generator.shuffle(rows)
            parsed = catalog.parse_catalog(self.encode(rows))
            self.validate(parsed)
            self.assertEqual(
                sorted(parsed, key=lambda r: r["claim_id"]),
                sorted(self.rows, key=lambda r: r["claim_id"]),
            )

    def test_generated_unknown_statuses_are_rejected(self):
        generator = random.Random(390)
        for _ in range(200):
            rows = copy.deepcopy(self.rows)
            rows[generator.randrange(len(rows))]["status"] = "unknown_" + str(
                generator.getrandbits(64)
            )
            with self.assertRaises(catalog.CatalogError):
                catalog.parse_catalog(self.encode(rows))

    def test_symlink_cannot_escape_input_root(self):
        with tempfile.TemporaryDirectory(dir=os.environ["TMPDIR"]) as directory:
            root = Path(directory) / "root"
            root.mkdir()
            outside = Path(directory) / "outside"
            outside.write_bytes(b"fixture")
            (root / "link").symlink_to(outside)
            with self.assertRaises(catalog.CatalogError):
                catalog.read_within(root, "link")
            (root / "inside").write_bytes(b"permitted")
            self.assertEqual(catalog.read_within(root, "inside"), b"permitted")

    def test_cli_missing_catalog_fails_without_success_output(self):
        with tempfile.TemporaryDirectory(dir=os.environ["TMPDIR"]) as directory:
            stdout, stderr = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                result = catalog.main(
                    ["--catalog", str(Path(directory) / "absent.tsv")]
                )
            self.assertEqual(result, 1)
            self.assertEqual(stdout.getvalue(), "")
            self.assertIn("Claim catalog error:", stderr.getvalue())


if __name__ == "__main__":
    unittest.main()
