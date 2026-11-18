#!/usr/bin/env python3
"""Generate the twelve escape kernels accepted by verify-escape.c."""

import argparse
from pathlib import Path
import re


def commute_product(function):
    return function.replace("fmul v35, v36", "fmul v36, v35")


def commute_sum(function):
    return function.replace("fadd v26, v27", "fadd v27, v26").replace(
        "fadd v43, v44", "fadd v44, v43"
    )


def phi_order(function):
    """Pass real/imaginary state before the squares, on both edges."""
    return (
        function.replace(
            "block4(v26: f64, v27: f64, v34: f64, v36: f64, v45: i64)",
            "block4(v34: f64, v36: f64, v26: f64, v27: f64, v45: i64)",
        )
        .replace("block4(v43, v44, v42, v39, v106)", "block4(v42, v39, v43, v44, v106)")
    )


def imag_first(function):
    real_start = function.index("    v40 = fsub")
    imag_start = function.index("    v35 = fadd", real_start)
    imag_end = function.index("\n", function.index("    v44 = fmul", imag_start)) + 1
    return (
        function[:real_start]
        + function[imag_start:imag_end]
        + "    jump block100\nblock100:\n"
        + function[real_start:imag_start]
        + function[imag_end:]
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    output = parser.parse_args().output
    source = Path(__file__).resolve().parents[1] / "counted-latch" / "escape.clif"
    functions = dict(re.findall(r"^function %(\w+)(.*?^})", source.read_text(), re.S | re.M))
    baseline = functions["a"]
    rotated = functions["g"]
    phi = phi_order(baseline)
    imag = imag_first(baseline)
    cases = {
        "original": baseline,
        "product": commute_product(baseline),
        "sum": commute_sum(baseline),
        "product_sum": commute_sum(commute_product(baseline)),
        "rotated": rotated,
        "rotated_product": commute_product(rotated),
        "rotated_sum": commute_sum(rotated),
        "rotated_product_sum": commute_sum(commute_product(rotated)),
        "phi_order": phi,
        "phi_order_product": commute_product(phi),
        "imag_first": imag,
        "imag_first_product": commute_product(imag),
    }
    output.write_text("\n\n".join(f"function %{name}{body}" for name, body in cases.items()) + "\n")


if __name__ == "__main__":
    main()
