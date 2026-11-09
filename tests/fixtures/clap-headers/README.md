# CLAP headers for ABI integration tests

Unmodified header subset and license from [CLAP 1.2.10](https://github.com/free-audio/clap/tree/1.2.10), retrieved from that release tag. The canonical source paths are `include/clap/` and `LICENSE`; `SHA256SUMS` records the exact vendored bytes. CLAP is MIT licensed; the complete license is in `LICENSE`.

These headers are test-only ABI oracles, included by `tests/fixtures/clap-abi/host.c`. They are not compiled into the Plenty compiler/runtime or shipped release archives, so they do not add a runtime dependency or release notice. The fixture is an ABI service using the actual CLAP entry and event-list structures, not an audio plugin.

To update, copy the same upstream paths from the chosen explicit release tag, retain its license, regenerate `SHA256SUMS`, update this version, and run the native CLAP integration test. Review native sizes, offsets and callback signatures together with the Plenty binding.
