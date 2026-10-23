# Module cache investigation

`run.py` uses existing scalar C exports to test separately compiled module
artifacts. `check_dependencies.py` demonstrates a body-derived borrowing fact
that a real module interface must track. Neither modifies Plenty's compiler or
enables production module caching.

See the [report](../../book/src/development/module-cache-investigation.md) for
commands, measurements, validation and limitations. Python 3, GNU `time`, `ar`,
the native linker and a built Plenty compiler are required. All generated
sources/artifacts live in temporary directories; only the requested JSON report
persists. Run with ordinary Python assertions enabled (not `python -O`).
