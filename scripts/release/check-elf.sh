#!/usr/bin/env bash
# Reject release executables that need more than the published baseline.
#
# Usage: check-elf.sh [--dataframe-engine] ELF...
#
# Baseline: x86-64 GNU/Linux, glibc 2.28, libgcc_s from GCC 4.2 or later.
# Each executable must be a dynamically linked x86-64 ELF using the standard
# interpreter, need only the libraries below, carry no RPATH/RUNPATH, and
# reference no GLIBC_ symbol version newer than 2.28 or GCC_ newer than 4.2.0.
# This complements running the archive on glibc 2.28; it does not replace it.
set -euo pipefail

max_glibc=2.28
max_gcc=4.2.0
allowed_needed="libc.so.6 libm.so.6 libgcc_s.so.1 libpthread.so.0 libdl.so.2 librt.so.1 libutil.so.1 ld-linux-x86-64.so.2"
engine=0
if [ "${1:-}" = --dataframe-engine ]; then engine=1; shift; fi
[ "$#" -gt 0 ] || { echo "check-elf: no inputs" >&2; exit 2; }

newer_than() { # newer_than A B: A > B as versions
    [ "$1" != "$2" ] && [ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -n1)" = "$1" ]
}

status=0
for exe in "$@"; do
    header="$(readelf -h "$exe")"
    grep -q 'Machine:[[:space:]]*Advanced Micro Devices X86-64' <<<"$header" \
        || { echo "$exe: not an x86-64 ELF" >&2; status=1; continue; }
    interp="$(readelf -l "$exe" | sed -n 's/.*\[Requesting program interpreter: \(.*\)\]/\1/p')"
    if [ "$engine" = 1 ]; then
        [ -z "$interp" ] && grep -Eq 'Type:[[:space:]]*DYN' <<<"$header" \
            || { echo "$exe: expected a shared library without an interpreter" >&2; status=1; }
    else
        [ "$interp" = /lib64/ld-linux-x86-64.so.2 ] \
            || { echo "$exe: interpreter '$interp', expected /lib64/ld-linux-x86-64.so.2" >&2; status=1; }
    fi
    dynamic="$(readelf -d "$exe")"
    if [ "$engine" = 1 ]; then
        soname="$(sed -n 's/.*(SONAME).*\[\(.*\)\]/\1/p' <<<"$dynamic")"
        [ "$soname" = libplenty_dataframe_v1.so ] \
            || { echo "$exe: unexpected dataframe SONAME '$soname'" >&2; status=1; }
    fi
    if grep -Eq '\((RPATH|RUNPATH)\)' <<<"$dynamic"; then
        echo "$exe: has RPATH/RUNPATH" >&2; status=1
    fi
    needed="$(sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p' <<<"$dynamic")"
    for lib in $needed; do
        case " $allowed_needed " in
            *" $lib "*) ;;
            *) echo "$exe: needs $lib, outside the baseline" >&2; status=1 ;;
        esac
    done
    versions="$(readelf -V "$exe" | grep -Eo '(GLIBC|GCC)_[0-9][0-9.]*' | sort -u)"
    glibc="$(grep '^GLIBC_' <<<"$versions" | sed 's/^GLIBC_//' | sort -V | tail -n1)"
    gcc="$( (grep '^GCC_' <<<"$versions" || true) | sed 's/^GCC_//' | sort -V | tail -n1)"
    if newer_than "$glibc" "$max_glibc"; then
        echo "$exe: needs GLIBC_$glibc, newer than the GLIBC_$max_glibc baseline:" >&2
        readelf -W --dyn-syms "$exe" | grep -E "@GLIBC_[0-9.]+" | awk '{print $8}' \
            | while IFS=@ read -r symbol version; do
                if newer_than "${version#GLIBC_}" "$max_glibc"; then echo "  $symbol@$version" >&2; fi
            done || true
        status=1
    fi
    if [ -n "$gcc" ] && newer_than "$gcc" "$max_gcc"; then
        echo "$exe: needs GCC_$gcc from libgcc_s, newer than GCC_$max_gcc" >&2; status=1
    fi
    echo "$exe: needs [${needed//$'\n'/ }]; newest GLIBC_$glibc${gcc:+, GCC_$gcc}"
done
exit $status
