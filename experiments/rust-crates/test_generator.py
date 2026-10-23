"""Unsupported syntax must fail rather than silently change the program."""
import unittest
import run


class GrammarTests(unittest.TestCase):
    def test_reject_control_flow(self):
        with self.assertRaises(ValueError):
            run.parse("def run(value: i64) -> i64:\n    if value:\n        value\n")

    def test_reject_unknown_api(self):
        with self.assertRaises(ValueError):
            run.parse("def run(value: i64) -> i64:\n    answer = unknown(value)\n    answer\n")

    def test_literal_ampersand_survives_borrow_normalization(self):
        plan = run.parse('def run(value: i64) -> i64:\n    answer = char_count("a&mut b")\n    answer\n')
        self.assertEqual(plan["operations"][0]["args"][0]["text"], "a&mut b")


if __name__ == "__main__":
    unittest.main()
