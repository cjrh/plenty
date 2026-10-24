# Function-alignment follow-up

Issue #437's [placement probe](indexed-access.md#bounded-placement-check-issue-437)
found a repeatable scan-throughput difference with identical function bytes.
The next experiment tested a small potential remedy: raise the alignment of
every generated function to 16, 32 or 64 bytes.

**Do not adopt a blanket function-alignment change from these results.** It
improves some placements and worsens others. In particular, 64-byte alignment
makes the one-column scan consistently slower than its best unaligned placement.
This follow-up changes the experiment and documentation, not compiler behavior.
Issue #437 remains open at low priority.

## Available backend control

The pinned Cranelift 0.131.1 exposes `log2_min_function_alignment`. Its x86-64
backend specifies a minimum alignment of one byte and a preferred alignment of
32 bytes, but `ObjectModule` uses the compiled buffer's required alignment.
Our baseline object therefore has byte-aligned functions (some sections need
eight-byte alignment for constants). Preferred alignment is not a guarantee
that the object linker places every function on a 32-byte boundary.

This version has no public loop-header alignment control. Its
`bb_padding_log2_minus_one` option inserts dummy zero bytes for code-generator
stress testing; it is not a production loop-alignment setting. These observations
come from the pinned crate's `shared/settings.rs`, `isa/x64/inst/mod.rs`,
`machinst/buffer.rs`, and the object backend, rather than an assumption about
what another version or Wasmtime does.

## Controlled comparison

The compiler was freshly built from `68108734` without allocation instrumentation.
Each workload was compiled once. GNU `objcopy` then raised all function-section
alignments, and GNU ld linked each variant at eight requested text addresses.
The baseline retains the original object, including constant-pool alignment.
The probe verifies unchanged function-section contents and actual function
addresses. It never inserts instructions or changes arithmetic semantics.

Measurements use the AMD Ryzen 7 7840HS, pinned to CPU 8, after the compiler build
finished. Host-visible process checks found no competing builds around the
measurement runs. Each sweep shuffles variant order with seed 437, excludes one
warmup, and takes three samples per variant; every checksum passed. This is a
small single-machine rejection test, not a representative architecture survey.

Ranges below are the minimum and maximum **per-placement median cycles/row**.
Counters cover startup, construction, repeated calls and teardown; internal
timers cover repeated calls. In the short scan, call/setup costs are substantial.

| Workload | Original alignment | 16 bytes | 32 bytes | 64 bytes |
| --- | ---: | ---: | ---: | ---: |
| One-column fold, 8,192 rows | 2.04–4.07 | 2.04–3.06 | 2.04–3.05 | 3.04–3.11 |
| Two-column filtered fold, 8,192 rows | 11.17–11.72 | 10.87–12.71 | 10.76–12.24 | 10.79–10.91 |
| Checked scalar range sum, 8,192 iterations | 3.01–3.02 | 2.01–3.02 | 2.01–3.02 | 3.01–3.02 |
| One-column fold, 8 rows | 6.60–8.68 | 6.73–7.70 | 6.48–7.91 | 7.85–7.90 |

The one-column scan's first sweep took 0.409–0.819 ns/row with original
alignment, 0.409–0.618 with 32-byte alignment, and 0.609–0.624 with 64-byte
alignment. A second complete sweep reproduced the same groups: 2.04–4.06
cycles/row originally, 2.04–3.06 at 32 bytes, and 3.04–3.06 at 64 bytes. Use
these fresh within-run comparisons; earlier reports were measured at different CPU speeds.
Across the first sweep's alignments, retired instructions varied by fewer than
200 out of about 1.33 billion. Identical instruction counts are not identical
throughput.

The scan function remains 294 bytes. At matching requested text addresses,
16-byte alignment adds 80 bytes to the linked text section, 32 adds 192–208,
and 64 adds 416–464. Executable file size happens to remain unchanged because
of existing segment padding. Across the other controls, text growth remains
under 500 bytes; throughput tradeoffs, not this small size cost, reject the
blanket change. Larger applications could accumulate more padding.

## Interpretation and reproduction

Function alignment fixes the function's starting boundary, not the hot loop's
boundary within it. The one-column loop has a condition at function offset
`0xc6` and a body at `0xfd`; aligning the function to 64 bytes does not align
either block. Moving both blocks can improve or worsen instruction delivery.
The exact CPU mechanism is still not isolated, and the probe moves other code
as well. These results do not justify a benchmark-specific offset or NOP patch.

A further fix would need loop-aware backend work and broader validation;
changing the global minimum alignment alone does not resolve this issue.
That exceeds this bounded follow-up. No new user-facing flag is proposed.

The probe and all samples are in `experiments/columnar-list/plenty/`:
`probe_scan_layout.py` and `alignment-policy-measurements.json`. Requires x86-64
Linux, GNU binutils, a C linker driver, and access to user-mode perf counters.
After building the compiler, run the following sequentially without competing
builds. Repeat the first command with a new output directory to check stability.

```sh
python3 -B experiments/columnar-list/plenty/probe_scan_layout.py target/release/plenty \
  --out /tmp/alignment-one --alignments 1 16 32 64
python3 -B experiments/columnar-list/plenty/probe_scan_layout.py target/release/plenty \
  --out /tmp/alignment-two --alignments 1 16 32 64 --kernel two --repeats 5000
python3 -B experiments/columnar-list/plenty/probe_scan_layout.py target/release/plenty \
  --out /tmp/alignment-scalar --alignments 1 16 32 64 --kernel scalar
python3 -B experiments/columnar-list/plenty/probe_scan_layout.py target/release/plenty \
  --out /tmp/alignment-tiny --alignments 1 16 32 64 --rows 8 --repeats 1000000
```
