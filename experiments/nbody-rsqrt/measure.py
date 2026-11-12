"""Interleaved n-body timing: three variants x two CPU levels x eight layouts.

The layouts add 0 to 7 statements to `energy`, as experiments/nbody-spills/
layouts.sh does, so a difference is not a code placement effect (issue #437).
Usage: measure.py --build; then measure.py [CORE] [ROUNDS].
"""
import json, os, re, statistics, subprocess, sys, time
root = os.path.abspath(os.path.join(os.path.dirname(__file__), "../.."))
work = f"{root}/target/nbody-rsqrt"
os.makedirs(work, exist_ok=True)
compiler = f"{root}/target/release/plenty"
core = sys.argv[1] if len(sys.argv) > 1 else "3"
rounds = int(sys.argv[2]) if len(sys.argv) > 2 else 5
steps = 5_000_000
variants = {
    "scalar": f"{root}/benchmarks/game/n-body/plenty/nbody.plenty",
    "exact": f"{root}/experiments/nbody-rsqrt/nbody_exact.plenty",
    "estimate": f"{root}/benchmarks/game/n-body/plenty/nbody_simd.plenty",
}
levels = ["x86-64", "x86-64-v3"]
env = dict(os.environ, PLENTY_CACHE="off")

def layout(src, n):
    out = []
    for line in open(src):
        if line.rstrip("\n") == "    Ok(e)":
            out += [f"    e += 0.0 * (dt + {i}.0)\n" for i in range(n)]
        out.append(line)
    text = "".join(out)
    text = re.sub(r"^def energy\(bodies: &(c*list)\[Body\]\)", r"def energy(bodies: &\1[Body], dt: f64)", text, flags=re.M)
    return text.replace("energy(&bodies)", "energy(&bodies, 0.01)")

bins = []
if "--build" in sys.argv:
    for v, src in variants.items():
        for n in range(8):
            path = f"{work}/{v}_{n}.plenty"
            open(path, "w").write(layout(src, n))
            for lv in levels:
                exe = f"{work}/{v}_{lv}_{n}"
                subprocess.run([compiler, "--cpu", lv, "--compile", path, "-o", exe], check=True, env=env)
    sys.exit(0)
for v in variants:
    for lv in levels:
        for n in range(8):
            bins.append((v, lv, n, f"{work}/{v}_{lv}_{n}"))
results = {}
for r in range(rounds):
    order = bins if r % 2 == 0 else bins[::-1]
    for v, lv, n, exe in order:
        t = time.perf_counter()
        p = subprocess.run(["perf", "stat", "-x,", "-e", "cycles:u,instructions:u", "taskset", "-c", core, exe, str(steps)],
                           capture_output=True, text=True, check=True)
        wall = time.perf_counter() - t
        vals = [float(l.split(",")[0]) for l in p.stderr.strip().splitlines() if "," in l]
        results.setdefault(f"{v} {lv}", []).append((vals[0] / steps, vals[1] / steps, wall * 1e3, n))
json.dump(results, open(f"{work}/results.json", "w"))
print("variant level: median cycles/step [min-max of layout medians], instructions/step, median wall ms")
for k, rows in results.items():
    cyc = statistics.median(r[0] for r in rows)
    per_layout = [statistics.median(r[0] for r in rows if r[3] == n) for n in range(8)]
    print(f"{k}: {cyc:.1f} [{min(per_layout):.1f}-{max(per_layout):.1f}] {statistics.median(r[1] for r in rows):.1f} {statistics.median(r[2] for r in rows):.0f}")
