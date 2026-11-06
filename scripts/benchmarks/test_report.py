"""Contract checks for benchmark summaries and future-regression comparisons."""
import math
import unittest

from scripts.benchmarks.report import (
    build_summary, compare_reports, render_comparison, render_markdown, summarize,
)


def metric(values=None, **extra):
    return dict(id="action.space.switch", unit="ms", scope="command_to_draw_submission", status="available", samples=values if values is not None else [10.] * 20, **extra)


def report(values=None, count=3):
    return {
        "schema_version": 1, "suite_version": 1, "status": "passed",
        "metadata": {
            "hardware": {"model": "Fixture Mac", "os_version": "27.0", "architecture": "arm64", "logical_cpus": 8, "memory_bytes": 16000000000, "webkit_version": "123"},
            "binary_sha256": "first-binary", "git_revision": "baseline", "git_dirty": False,
            "fixture_sha256": "fixture-v1", "benchmark_sha256": "benchmark-v1",
            "configuration": {"iterations": 20, "warmup": 2, "session_sizes": ["small"], "suites": ["actions"], "app_nap": "suppressed"},
            "conditions": {"power_source": "AC", "thermal": "nominal"},
        },
        "runs": [{"session_size": "small", "repeat": i + 1, "status": "passed", "metrics": [metric(values)], "quality": {"active": True}, "resources": [], "energy": {"status": "unavailable", "reason": "AC power", "scope": "whole_device"}} for i in range(count)],
    }


class SummaryTests(unittest.TestCase):
    def test_nearest_rank_and_mean(self):
        stats = summarize(range(1, 21))
        self.assertEqual((stats["n"], stats["p50"], stats["p95"], stats["max"], stats["mean"]), (20, 10, 19, 20, 10.5))
        self.assertEqual(summarize([9])["p95"], 9)
        self.assertIsNone(summarize([])["p50"])

    def test_non_numeric_or_non_finite_data_is_invalid(self):
        for value in (True, "12", None, math.nan, math.inf, -math.inf):
            with self.subTest(value=value), self.assertRaises(ValueError):
                summarize([value])

    def test_pooled_summary_preserves_run_medians(self):
        data = report([1., 2.])
        data["runs"][1]["metrics"][0]["samples"] = [3., 4.]
        row = build_summary(data)["metrics"][0]
        self.assertEqual(row["summary"]["n"], 6)
        self.assertEqual(row["run_medians"], [1., 3., 1.])
        self.assertEqual(row["run_count"], 3)
        self.assertFalse(row["confidence"]["sufficient"])
        self.assertIn("fewer than 20 raw samples", row["confidence"]["reasons"])

    def test_different_units_scopes_and_sessions_are_not_pooled(self):
        data = report(count=1)
        second = metric([1000.])
        second["unit"] = "us"
        third = metric([20.])
        third["scope"] = "model_only"
        data["runs"][0]["metrics"] += [second, third]
        data["runs"].append(dict(data["runs"][0], session_size="large"))
        rows = build_summary(data)["metrics"]
        self.assertEqual(len(rows), 6)
        self.assertEqual({(r["session_size"], r["unit"], r["scope"]) for r in rows}, {(s, u, c) for s in ("small", "large") for u, c in [("ms", "command_to_draw_submission"), ("us", "command_to_draw_submission"), ("ms", "model_only")]})

    def test_unavailable_and_failed_runs_never_enter_statistics(self):
        data = report()
        data["runs"][0]["metrics"][0].update(status="unavailable", reason="no frame callbacks", samples=[10000.])
        data["runs"][1]["status"] = "failed"
        row = build_summary(data)["metrics"][0]
        self.assertEqual(row["summary"]["n"], 20)
        self.assertEqual(row["status"], "partial")
        self.assertEqual(row["available_run_count"], 1)
        self.assertIn("no frame callbacks", " ".join(row["reasons"]))

    def test_provided_summary_is_not_a_replacement_for_raw_samples(self):
        data = report([])
        data["runs"][0]["metrics"][0]["summary"] = {"n": 100, "p50": 9}
        row = build_summary(data)["metrics"][0]
        self.assertEqual(row["status"], "unavailable")
        self.assertEqual(row["summary"]["n"], 0)


class ComparisonTests(unittest.TestCase):
    def test_binary_and_git_changes_are_expected(self):
        first, second = report(), report()
        second["metadata"].update(binary_sha256="new-binary", git_revision="next", git_dirty=True)
        result = compare_reports(first, second)
        self.assertTrue(result["compatible"])
        self.assertEqual(result["status"], "unchanged")
        self.assertTrue(result["confidence"]["sufficient"])

    def test_all_hardware_configuration_fixture_and_condition_mismatches_block(self):
        mutations = [
            ("hardware", "webkit_version", "124"), ("hardware", "os_version", "28"),
            ("hardware", "model", "Different Mac"), ("configuration", "iterations", 40),
            ("conditions", "power_source", "Battery"), ("conditions", "thermal", "serious"),
        ]
        for section, key, value in mutations:
            with self.subTest(section=section, key=key):
                other = report()
                other["metadata"][section][key] = value
                self.assertEqual(compare_reports(report(), other)["status"], "incompatible")
        for key in ("fixture_sha256", "benchmark_sha256"):
            other = report()
            other["metadata"][key] = "changed"
            self.assertFalse(compare_reports(report(), other)["compatible"])
        for key in ("schema_version", "suite_version"):
            other = report()
            other[key] = 2
            self.assertFalse(compare_reports(report(), other)["compatible"])

    def test_missing_provenance_is_not_equal_provenance(self):
        first, second = report(), report()
        del first["metadata"]["fixture_sha256"]
        del second["metadata"]["fixture_sha256"]
        self.assertFalse(compare_reports(first, second)["compatible"])

    def test_versions_require_positive_integers_on_both_sides(self):
        for side in (0, 1):
            for key in ("schema_version", "suite_version"):
                for value in (True, False, 1.0, "1", None, 0, -1):
                    with self.subTest(side=side, key=key, value=value):
                        pair = [report(), report()]
                        pair[side][key] = value
                        self.assertEqual(compare_reports(*pair)["status"], "incompatible")

    def test_unit_scope_and_direction_changes_block(self):
        for key, value in (("unit", "us"), ("scope", "model_only"), ("direction", "higher")):
            other = report()
            for run in other["runs"]:
                run["metrics"][0][key] = value
            self.assertEqual(compare_reports(report(), other)["status"], "incompatible")

    def test_ms_regression_requires_relative_and_absolute_thresholds(self):
        self.assertEqual(compare_reports(report([1.] * 20), report([1.5] * 20))["status"], "unchanged")
        self.assertEqual(compare_reports(report([100.] * 20), report([105.] * 20))["status"], "unchanged")
        result = compare_reports(report(), report([12.] * 20))
        self.assertEqual(result["status"], "regression")
        self.assertEqual(result["comparisons"][0]["changes"]["p95"]["percent"], 20)
        self.assertEqual(compare_reports(report(), report([8.] * 20))["status"], "improved")

    def test_tail_regressions_are_detected_even_with_unchanged_median(self):
        values = [10.] * 18 + [40.] * 2
        result = compare_reports(report(), report(values))
        self.assertEqual(result["status"], "regression")
        self.assertFalse(result["comparisons"][0]["changes"]["p50"]["regressed"])
        self.assertTrue(result["comparisons"][0]["changes"]["p95"]["regressed"])

    def test_low_confidence_changes_are_suspected_non_gating(self):
        result = compare_reports(report(count=1), report([12.] * 20, count=1))
        self.assertEqual(result["status"], "suspected_regression")
        self.assertEqual(result["regressions"], [])
        self.assertFalse(result["confidence"]["sufficient"])

    def test_non_ms_relative_threshold_and_higher_is_better(self):
        first, second = report([60.] * 20), report([40.] * 20)
        for item in (first, second):
            for run in item["runs"]:
                run["metrics"][0]["unit"] = "fps"
        self.assertEqual(compare_reports(first, second)["status"], "regression")
        self.assertEqual(compare_reports(second, first)["status"], "improved")

    def test_zero_baseline_does_not_emit_infinity(self):
        result = compare_reports(report([0.] * 20), report([2.] * 20))
        self.assertEqual(result["status"], "regression")
        self.assertIsNone(result["comparisons"][0]["changes"]["p50"]["percent"])
        __import__("json").dumps(result, allow_nan=False)

    def test_missing_required_metrics_and_failed_report_are_incomplete(self):
        other = report()
        other["runs"][0]["metrics"] = []
        self.assertEqual(compare_reports(report(), other)["status"], "incomplete")
        other = report()
        other["status"] = "failed"
        self.assertEqual(compare_reports(report(), other)["status"], "incomplete")
        other = report()
        other["runs"][0]["quality"] = {"valid": False, "reason": "thermal transition"}
        result = compare_reports(report(), other)
        self.assertEqual(result["status"], "incomplete")
        self.assertFalse(result["quality"]["candidate"][0]["quality"]["valid"])

    def test_optional_unavailable_does_not_block_supported_metrics(self):
        first, second = report(), report()
        for item in (first, second):
            for run in item["runs"]:
                run["metrics"].append(dict(id="web.paint", unit="ms", scope="web_performance", status="unavailable", reason="API unsupported", required=False, samples=[]))
        result = compare_reports(first, second)
        self.assertEqual(result["status"], "unchanged")
        self.assertEqual(result["comparisons"][1]["status"], "unavailable")

    def test_no_measurements_never_reports_unchanged(self):
        first, second = report([]), report([])
        for item in (first, second):
            for run in item["runs"]:
                run["metrics"][0]["required"] = False
        self.assertEqual(compare_reports(first, second)["status"], "incomplete")

    def test_invalid_thresholds_rejected(self):
        for value in (math.nan, math.inf, -1., True):
            with self.subTest(value=value), self.assertRaises(ValueError):
                compare_reports(report(), report(), relative_threshold=value)

    def test_markdown_preserves_availability_energy_and_provenance(self):
        data = report(count=1)
        data["runs"][0]["metrics"][0].update(status="unavailable", reason="No rAF callbacks | occluded")
        text = render_markdown(data)
        self.assertIn("No rAF callbacks \\| occluded", text)
        self.assertIn("whole_device", text)
        self.assertIn("not converted into battery energy", text)
        self.assertIn("first-binary", text)
        self.assertIn("unavailable", text)
        comparison = render_comparison(compare_reports(report(), report([12.] * 20)))
        self.assertIn("regression", comparison)
        self.assertIn("12.000", comparison)


if __name__ == "__main__":
    unittest.main()
