#!/usr/bin/env python3
"""Replaces build-directory paths that C dependencies bake into the binary.

    scrub-build-paths.py <binary> <prefix> [<prefix> ...]

--remap-path-prefix only affects Rust code. Vendored OpenSSL (via libssh2-sys)
compiles its install prefix into string constants such as
"<target>/.../openssl-build/install/lib/engines-3" (ENGINESDIR, MODULESDIR), which
would reveal the build machine's home folder. Each NUL-terminated string that
starts with one of the prefixes is overwritten in place with "/var/empty" (an
empty, root-owned directory on macOS) and padded with NULs, so no offsets change.
These directories are only used when OpenSSL is explicitly asked to load an
engine or provider module by name, which Manta never does.

Run before code signing (it changes the file). Prints how many strings it replaced.
"""
import sys

NEUTRAL = b"/var/empty"


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    path, prefixes = sys.argv[1], [p.encode() for p in sys.argv[2:] if p]
    data = bytearray(open(path, "rb").read())
    count = 0
    for prefix in prefixes:
        start = data.find(prefix)
        while start != -1:
            end = data.find(b"\0", start)
            if end == -1:
                end = len(data)
            length = end - start
            if length >= len(NEUTRAL):
                data[start:end] = NEUTRAL + b"\0" * (length - len(NEUTRAL))
            else:
                data[start:end] = b"\0" * length
            count += 1
            start = data.find(prefix, start + 1)
    if count:
        open(path, "wb").write(data)
    print(count)


if __name__ == "__main__":
    main()
