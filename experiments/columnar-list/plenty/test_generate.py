"""The spike's narrow input contract must fail closed, not silently omit fields."""
import unittest

from generate import generate, schema


class SchemaTests(unittest.TestCase):
    def test_preserves_order_and_scalar_types(self):
        self.assertEqual(schema("pub class R:\n    pub z: f32\n    pub a: u8\n"),
                         ("R", [("z", "f32"), ("a", "u8")]))

    def test_rejects_nonprimitive_and_behavior(self):
        for field in ["str", "list[i64]", "Option[i64]", "R", "&i64", "Unknown"]:
            with self.subTest(field=field), self.assertRaises(ValueError):
                generate(f"pub class R:\n    pub x: {field}\n")
        for source in ["pub class R:", "class R:\n    pub x: i64", "pub class R[T]:\n    pub x: i64",
                       "pub class R:\n    x: i64", "pub class R:\n    pub x: i64\n    pub x: f64",
                       "pub class R:\n    pub x: i64\n    def __del__(self: &mut R) -> ():\n        ()"]:
            with self.subTest(source=source), self.assertRaises(ValueError):
                generate(source)


if __name__ == "__main__":
    unittest.main()
