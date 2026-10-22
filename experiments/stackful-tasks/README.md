# Fixed-stack cooperative task experiment

Standalone Linux/glibc C spike for the exploratory
[Go-style task proposal](../../book/src/proposals/stackful-tasks.md).
The [evidence page](../../book/src/proposals/stackful-tasks/evidence.md) explains
the checks and their limits. No Plenty production syntax/runtime is changed.

Run from this checkout:

```sh
python3 -B experiments/stackful-tasks/test_spike.py
CC=clang SANITIZE=1 python3 -B experiments/stackful-tasks/test_spike.py
```

Requires a C11 compiler and Linux/glibc `ucontext`; sanitizer mode adds UBSan,
not complete alternate-stack memory checking. Each task has a fixed 64 KiB
usable stack. No M:N scheduling, preemption, stack growth, real I/O, migration,
forced cancellation or safe stack-overflow recovery is implemented.
