# Brotli 1.2.1

Unmodified `c/common`, `c/dec`, `c/enc`, `c/include` and `LICENSE` from the official
google/brotli `v1.2.1` tag archive. The command-line tool and other languages,
tests, build systems and documentation are omitted.

Archive SHA-256:
`98d35d11a78dc453aebb7d10c3748f3f73ff78c8267c0dc697b8631f67bf7e8f`.

The streaming encoder and decoder use runtime allocation/free callbacks. Null
allocation results become `AllocError`, and destruction frees failed codec states.
The explicit `BROTLI_ENCODER_CLEANUP_ON_OOM` build flag is required: upstream
otherwise selects `BROTLI_ENCODER_EXIT_ON_OOM` and exits on allocation failure.
Large-window extensions and shared custom
dictionaries are disabled; the wire format is RFC 7932.

Version 1.2.1 includes the decoder block-switch validation fix
(`BROTLI_DECODER_ERROR_FORMAT_BLOCK_SWITCH`, CVE-2026-111497). Do not downgrade
to an unpatched 1.2.0 or earlier source. Updating requires reviewing upstream
security changes, replacing the source and license together, updating this
digest and release notices, and rerunning interop, malformed-input, tiny-buffer,
allocation-failure, cleanup, native-link and installed-compiler tests.

`native/brotli_build.rs` builds the same source for both Cargo runtime tests and
the compiler's embedded runtime archives. Function/data sections allow the
linker to discard the codec when a program does not use compression. No system
Brotli package or runtime shared library is needed.
