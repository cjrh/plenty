#!/usr/bin/env python3
"""Reject macOS or Windows release executables that need more than the baseline.

Usage: check-native.py --target TRIPLE [--macos-min X.Y] EXECUTABLE...

aarch64-apple-darwin: a thin arm64 Mach-O executable with a code signature,
built for at most --macos-min, loading only system libraries from /usr/lib or
/System/Library, and without LC_RPATH. A Homebrew or toolchain dylib would
make the archive depend on the build runner.

x86_64-pc-windows-msvc: an x86-64 PE32+ console executable importing only
Windows system DLLs and the MSVC runtime, which the documented Visual Studio
Build Tools prerequisite installs.

The parsers read only the headers they check, so tests can use synthetic
images and the audit needs no Xcode or Visual Studio tools.
"""
import argparse
from pathlib import Path
import struct
import sys

MACHO_MAGIC = 0xFEEDFACF
FAT_MAGICS = {0xCAFEBABE, 0xBEBAFECA, 0xCAFEBABF, 0xBFBAFECA}
CPU_ARM64 = 0x0100000C
MH_EXECUTE = 2
LC_DYLIBS = {0xC: "LC_LOAD_DYLIB", 0x80000018: "LC_LOAD_WEAK_DYLIB",
             0x8000001F: "LC_REEXPORT_DYLIB", 0x20: "LC_LAZY_LOAD_DYLIB",
             0x80000023: "LC_LOAD_UPWARD_DYLIB"}
LC_RPATH = 0x8000001C
LC_CODE_SIGNATURE = 0x1D
LC_VERSION_MIN_MACOSX = 0x24
LC_BUILD_VERSION = 0x32
PLATFORM_MACOS = 1
MACOS_SYSTEM = ("/usr/lib/", "/System/Library/")

PE_MACHINE_AMD64 = 0x8664
PE32_PLUS = 0x20B
SUBSYSTEM_CONSOLE = 3
WINDOWS_SYSTEM = {
    "advapi32.dll", "bcrypt.dll", "bcryptprimitives.dll", "crypt32.dll", "dbghelp.dll",
    "dnsapi.dll", "iphlpapi.dll", "kernel32.dll", "kernelbase.dll", "mswsock.dll",
    "ncrypt.dll", "netapi32.dll", "ntdll.dll", "ole32.dll", "oleaut32.dll", "powrprof.dll",
    "psapi.dll", "rpcrt4.dll", "secur32.dll", "shell32.dll", "shlwapi.dll", "sspicli.dll",
    "user32.dll", "userenv.dll", "version.dll", "winmm.dll", "wintrust.dll", "ws2_32.dll",
    "ucrtbase.dll",
}
WINDOWS_PREFIXES = ("api-ms-win-", "ext-ms-win-", "vcruntime140", "msvcp140")


def version_text(packed):
    return f"{packed >> 16}.{(packed >> 8) & 0xFF}.{packed & 0xFF}"


def version_number(text):
    parts = [int(p) for p in text.split(".")] + [0, 0]
    return (parts[0] << 16) | (parts[1] << 8) | parts[2]


def check_macho(data, macos_min):
    """Return (problems, summary) for a Mach-O image."""
    if len(data) < 32:
        return ["truncated Mach-O header"], ""
    magic = struct.unpack_from("<I", data)[0]
    if struct.unpack_from(">I", data)[0] in FAT_MAGICS:
        return ["universal (fat) binary; expected thin arm64"], ""
    if magic != MACHO_MAGIC:
        return ["not a little-endian 64-bit Mach-O image"], ""
    cputype, _, filetype, ncmds, _, _ = struct.unpack_from("<iIIIII", data, 4)
    problems = []
    if cputype != CPU_ARM64:
        problems.append(f"CPU type {cputype:#x}, expected arm64")
    if filetype != MH_EXECUTE:
        problems.append(f"file type {filetype}, expected an executable")
    dylibs, minimum, signed = [], None, False
    offset = 32
    for _ in range(ncmds):
        if offset + 8 > len(data):
            problems.append("truncated load commands")
            break
        cmd, size = struct.unpack_from("<II", data, offset)
        if size < 8 or offset + size > len(data):
            problems.append("malformed load command")
            break
        if cmd in LC_DYLIBS:
            start = offset + struct.unpack_from("<I", data, offset + 8)[0]
            name = data[start:offset + size].split(b"\0", 1)[0].decode("utf-8", "replace")
            dylibs.append(name)
            if not name.startswith(MACOS_SYSTEM):
                problems.append(f"{LC_DYLIBS[cmd]} {name} is not a macOS system library")
        elif cmd == LC_RPATH:
            problems.append("has LC_RPATH")
        elif cmd == LC_CODE_SIGNATURE:
            signed = True
        elif cmd == LC_BUILD_VERSION:
            platform, minos = struct.unpack_from("<II", data, offset + 8)
            if platform != PLATFORM_MACOS:
                problems.append(f"build platform {platform}, expected macOS")
            minimum = minos
        elif cmd == LC_VERSION_MIN_MACOSX:
            minimum = struct.unpack_from("<I", data, offset + 8)[0]
        offset += size
    if not signed:
        problems.append("no code signature; arm64 macOS refuses unsigned executables")
    if minimum is None:
        problems.append("no minimum macOS version")
    elif minimum > version_number(macos_min):
        problems.append(f"needs macOS {version_text(minimum)}, newer than the {macos_min} baseline")
    summary = f"macOS {version_text(minimum) if minimum is not None else '?'}+; loads [{' '.join(dylibs)}]"
    return problems, summary


def pe_sections(data, table, count):
    return [struct.unpack_from("<8sIIII", data, table + 40 * i) for i in range(count)]


def pe_offset(sections, rva):
    for _, vsize, address, raw_size, raw in sections:
        if address <= rva < address + max(vsize, raw_size):
            return raw + rva - address
    raise ValueError(f"RVA {rva:#x} is outside every section")


def pe_text(data, sections, rva):
    start = pe_offset(sections, rva)
    return data[start:data.index(b"\0", start)].decode("ascii", "replace")


def check_pe(data):
    """Return (problems, summary) for a PE image."""
    if len(data) < 0x40 or data[:2] != b"MZ":
        return ["not a PE image"], ""
    header = struct.unpack_from("<I", data, 0x3C)[0]
    if data[header:header + 4] != b"PE\0\0":
        return ["missing PE signature"], ""
    machine, count = struct.unpack_from("<HH", data, header + 4)
    optional_size = struct.unpack_from("<H", data, header + 20)[0]
    optional = header + 24
    problems = []
    if machine != PE_MACHINE_AMD64:
        problems.append(f"machine {machine:#x}, expected x86-64")
    if struct.unpack_from("<H", data, optional)[0] != PE32_PLUS:
        return problems + ["expected PE32+"], ""
    subsystem = struct.unpack_from("<H", data, optional + 68)[0]
    if subsystem != SUBSYSTEM_CONSOLE:
        problems.append(f"subsystem {subsystem}, expected console")
    directories = struct.unpack_from("<I", data, optional + 108)[0]
    sections = pe_sections(data, optional + optional_size, count)
    dlls = []
    try:
        for index, width, name_field in ((1, 20, 12), (13, 32, 4)):  # imports, delay imports
            if directories <= index:
                continue
            rva, _ = struct.unpack_from("<II", data, optional + 112 + 8 * index)
            if not rva:
                continue
            entry = pe_offset(sections, rva)
            while any(data[entry:entry + width]):
                dlls.append(pe_text(data, sections, struct.unpack_from("<I", data, entry + name_field)[0]))
                entry += width
    except (ValueError, struct.error) as error:
        problems.append(f"malformed import table: {error}")
    for dll in dlls:
        lower = dll.lower()
        if lower not in WINDOWS_SYSTEM and not lower.startswith(WINDOWS_PREFIXES):
            problems.append(f"imports {dll}, outside Windows and the MSVC runtime")
    return problems, f"imports [{' '.join(dlls)}]"


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--target", required=True, choices=("aarch64-apple-darwin", "x86_64-pc-windows-msvc"))
    parser.add_argument("--macos-min", default="11.0")
    parser.add_argument("executables", nargs="+", type=Path)
    args = parser.parse_args(argv)
    status = 0
    for exe in args.executables:
        data = exe.read_bytes()
        if args.target == "aarch64-apple-darwin":
            problems, summary = check_macho(data, args.macos_min)
        else:
            problems, summary = check_pe(data)
        for problem in problems:
            print(f"{exe}: {problem}", file=sys.stderr)
        if problems:
            status = 1
        else:
            print(f"{exe}: {summary}")
    return status


if __name__ == "__main__":
    sys.exit(main())
