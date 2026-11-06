import json
from pathlib import Path
import tempfile
import time
import unittest
from unittest.mock import patch

from benchmarks.energy import (EnergyPrerequisiteError, EnergySession,
                               parse_battery, parse_power,
                               summarize_battery, summarize_power)


def sample(percent, seconds=0, source="battery", state="discharging"):
    result = parse_battery(f"Now drawing from '{'Battery Power' if source == 'battery' else 'AC Power'}'\n -InternalBattery-0\t{percent}%; {state}; present: true")
    return dict(result, elapsed_seconds=seconds)


class BatteryTests(unittest.TestCase):
    def test_source_is_required_and_not_discharging_is_not_discharge(self):
        self.assertTrue(sample(55)["discharging"])
        self.assertFalse(parse_battery("55%; discharging;")["available"])
        self.assertFalse(sample(100, source="ac", state="charged")["discharging"])
        self.assertFalse(sample(88, state="not discharging")["discharging"])
        self.assertIsNone(parse_battery("Now drawing from 'Battery Power'\n101%; discharging;")["charge_percent"])

    def test_coarse_zero_is_below_resolution_not_zero_rate(self):
        result = summarize_battery([sample(80), sample(80, 600)], 600, True)
        self.assertEqual(result["status"], "measured")
        self.assertEqual(result["charge_change_percentage_points"], 0)
        self.assertTrue(result["below_resolution"])
        self.assertIsNone(result["percent_points_per_hour"])

    def test_long_observed_rate_and_reverse_order(self):
        result = summarize_battery([sample(77, 1800), sample(80)], 1800, True)
        self.assertEqual(result["charge_change_percentage_points"], 3)
        self.assertEqual(result["percent_points_per_hour"], 6)
        self.assertEqual(result["scope"], "whole_device")
        self.assertNotIn("watt_hours", result)

    def test_rate_uses_actual_observation_interval(self):
        result = summarize_battery([sample(80, 10), sample(77, 3010)], 3010, True)
        self.assertEqual(result["observation_interval_seconds"], 3000)
        self.assertEqual(result["percent_points_per_hour"], 3.6)

    def test_ac_transition_invalidates_charge_result(self):
        result = summarize_battery([sample(80), sample(79, 100, "ac", "charging"), sample(78, 600)], 600, True)
        self.assertEqual(result["status"], "invalid")
        self.assertIsNone(result["charge_change_percentage_points"])
        self.assertEqual(len(result["power_states"]), 3)

    def test_unknown_and_increasing_gauge_do_not_become_consumption(self):
        self.assertEqual(summarize_battery([sample(80)], 1000, True)["status"], "unavailable")
        self.assertEqual(summarize_battery([sample(80), sample(81, 600)], 600, True)["status"], "invalid")
        self.assertEqual(summarize_battery([sample(80), sample(79, 60)], 60, True)["percent_points_per_hour"], None)


class PowerTests(unittest.TestCase):
    def test_units_and_rails_are_not_double_counted(self):
        text = """*** Sampled system activity (date) (1000.0ms elapsed) ***
CPU Power: 1000 mW
GPU Power: 0.5 W
ANE Power: 0 mW
Combined Power (CPU + GPU + ANE): 1500 mW
"""
        row = parse_power(text)
        self.assertEqual(row["watts"], {"cpu": 1, "gpu": .5, "ane": 0, "combined_cpu_gpu_ane": 1.5})
        result = summarize_power([row, row], 3)
        self.assertEqual(result["scope"], "whole_soc")
        self.assertEqual(result["components"]["combined_cpu_gpu_ane"]["estimated_joules"], 3)
        self.assertEqual(result["covered_seconds"], 2)
        self.assertAlmostEqual(result["coverage_fraction"], 2/3)
        self.assertNotIn("total_joules", result)

    def test_missing_interval_and_unsupported_output_are_unavailable(self):
        self.assertEqual(parse_power("CPU Power: 2 W")["status"], "unavailable")
        self.assertEqual(parse_power("(1000ms elapsed)\nEnergy Impact: 999")["status"], "unavailable")
        self.assertEqual(parse_power("(0ms elapsed)\nCPU Power: 2 W")["status"], "unavailable")
        self.assertEqual(summarize_power([], 10)["status"], "unavailable")


class CollectorTests(unittest.TestCase):
    @patch("benchmarks.energy.read_battery", return_value=sample(100, source="ac", state="charged"))
    def test_battery_prerequisite_fails_without_launching_sampler(self, read):
        with tempfile.TemporaryDirectory() as directory:
            session = EnergySession(Path(directory), battery=True, power=True)
            with self.assertRaises(EnergyPrerequisiteError):
                session.start()
            self.assertEqual(session._threads, [])
            self.assertEqual(session.stop()["battery"]["status"], "invalid")

    @patch("benchmarks.energy.read_battery", return_value=sample(100, source="ac", state="charged"))
    def test_not_requested_is_not_a_fabricated_measurement(self, read):
        with tempfile.TemporaryDirectory() as directory:
            session = EnergySession(Path(directory))
            session.start()
            result = session.stop()
            self.assertEqual(result, session.stop())
            self.assertEqual(result["battery"]["status"], "not_requested")
            self.assertEqual(result["soc_power"]["status"], "not_requested")
            json.dumps(result)
            with self.assertRaises(RuntimeError):
                session.start()

    @patch("benchmarks.energy.read_battery", return_value=sample(80))
    def test_sampler_failure_is_unavailable_and_worker_exits(self, read):
        with tempfile.TemporaryDirectory() as directory:
            failure = type("Result", (), {"returncode": 1, "stdout": "", "stderr": "sudo: a password is required"})()
            with patch("benchmarks.energy.subprocess.run", return_value=failure) as command:
                session = EnergySession(Path(directory), power=True)
                session.start()
                for thread in session._threads[1:]:
                    thread.join(timeout=1)
                result = session.stop()
            self.assertEqual(result["soc_power"]["status"], "unavailable")
            self.assertIn("noninteractive sudo", result["soc_power"]["reason"])
            self.assertTrue(all(not t.is_alive() for t in session._threads))
            self.assertIn("-n", command.call_args.args[0])
            self.assertEqual(command.call_args.args[0][-2:], ["--sample-count", "1"])

    @patch("benchmarks.energy.read_battery", return_value=sample(80))
    def test_sampling_limit_makes_battery_protocol_invalid(self, read):
        with tempfile.TemporaryDirectory() as directory, patch("benchmarks.energy.MAX_SESSION_SECONDS", 0):
            session = EnergySession(Path(directory), battery=True)
            session.start()
            for thread in session._threads:
                thread.join(timeout=2)
            result = session.stop()
            self.assertEqual(result["battery"]["status"], "invalid")
            self.assertIsNone(result["battery"]["charge_change_percentage_points"])
            self.assertIn("one-hour", result["battery"]["reason"])

    @patch("benchmarks.energy.read_battery", return_value=sample(80))
    def test_end_sample_is_not_backdated_or_integration_past_end(self, read):
        with tempfile.TemporaryDirectory() as directory:
            session = EnergySession(Path(directory))
            session.start()
            session.power = True
            now = time.monotonic()
            session._power_samples = [dict(parse_power("(1ms elapsed)\nCPU Power: 1000 mW"), ended_monotonic=now),
                                      dict(parse_power("(1000ms elapsed)\nCPU Power: 1000 mW"), ended_monotonic=now + 100)]
            result = session.stop()
            rows = [json.loads(line) for line in (Path(directory)/"battery.jsonl").read_text().splitlines()]
            self.assertEqual(rows[-1]["elapsed_seconds"], result["battery"]["duration_seconds"])
            self.assertEqual(result["soc_power"]["sample_count"], 1)
            self.assertEqual(result["soc_power"]["components"]["cpu"]["estimated_joules"], .001)


if __name__ == "__main__":
    unittest.main()
