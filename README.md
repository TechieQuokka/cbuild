# cman

A Cargo-like build tool for C, written in Rust.

C has excellent compilers and no default project workflow. `cman` supplies the missing
half: a manifest, a conventional layout, and a build that knows what it already built.

```console
$ cman new hello
     Created binary `hello` package

$ cd hello && cman run
   Compiling hello v0.1.0 (/home/you/hello)
    Finished `debug` profile [unoptimized + debuginfo] target(s) in 0.02s
     Running target/debug/hello
Hello, world!
```

## Status

Working MVP. Five commands, no dependency resolution, no registry. It builds a single
executable from the C files in `src/`, and it does that part properly — including header
dependency tracking, which is what hand-written Makefiles usually get wrong.

## Install

Requires a Rust toolchain (2024 edition, i.e. Rust 1.85+) and a C compiler.

```console
$ cargo install cman
```

Or from a checkout:

```console
$ cargo install --path .
```

To hack on it instead:

```console
$ cargo build && ./target/debug/cman --help
```

## Commands

| Command | What it does |
| --- | --- |
| `cman new <path>` | Create a project in a new directory |
| `cman init [path]` | Create a project in an existing directory, preserving any files already there |
| `cman build [--release]` | Compile and link, rebuilding only what changed |
| `cman run [--release] [-- args...]` | Build, then run the executable and forward its exit code |
| `cman check [--release]` | Report errors without producing object code (`-fsyntax-only`) |
| `cman clean [--release]` | Delete `target/`, or just `target/release/` |

`new` and `init` take `--name` to set a package name that differs from the directory name.

## Manifest

`cman.toml` marks the project root, the way `Cargo.toml` does. Commands work from any
subdirectory.

```toml
[package]
name = "hello"      # also the name of the produced executable
version = "0.1.0"
std = "c17"         # passed through as -std=c17
```

`version` and `std` are optional and default to `"0.1.0"` and `"c17"`. Unknown keys are
rejected rather than ignored, so a typo is reported instead of silently doing nothing.

## Layout

```
hello/
├── cman.toml
├── include/            # optional; added to the include path as -Iinclude
│   └── greet.h
├── src/                # every .c file here is compiled, recursively
│   ├── main.c
│   ├── greet.c
│   └── math/
│       └── add.c
└── target/             # owned entirely by cman
    ├── debug/
    │   ├── hello               # the executable
    │   ├── obj/
    │   │   ├── main.o
    │   │   ├── main.d          # header dependencies, from gcc -MMD
    │   │   └── math/add.o      # the src/ tree is mirrored, so nested names cannot collide
    │   └── .cman-fingerprint   # hash of the compiler flags used
    └── release/
```

`src/` and `include/` are both on the include path, so `#include "math/add.h"` works from
anywhere in the project.

Debug and release live in separate trees. Switching `--release` never invalidates the other
profile's objects, and the two executables coexist.

## How rebuilds are decided

The interesting part of a build tool is not invoking the compiler, it is knowing when not
to. A source file is recompiled when any of these holds:

- the object file is missing;
- the `.c` file is newer than the object;
- **any header the object actually depends on is newer than the object**;
- the compiler flags changed.

The third condition is the one that matters. `cman` passes `-MMD` so the compiler records
the headers each translation unit really included, then reads those `.d` files back on the
next build. Editing a header rebuilds exactly the objects that include it — directly or
transitively — and nothing else. Omitting this is the classic Makefile bug: the build
succeeds, skips the affected objects, and silently produces a stale binary.

The fourth condition covers what mtimes cannot see. Changing `std` in the manifest, or
switching compilers via `CC`, does not touch a single file, so `cman` hashes the flags and
stores the hash next to the artifacts. A mismatch forces a full rebuild.

Objects are removed before being recompiled. If the compiler dies partway through, there is
no half-written file left with a fresh timestamp to be mistaken for up to date. Linking
works the same way.

Independent files compile in parallel, up to the number of available cores. Diagnostics are
buffered and printed in source order, so concurrent output never interleaves and the build
log is identical whether it ran on one core or sixteen.

## Choosing a compiler

`cman` uses `$CC`, falling back to `cc`.

```console
$ CC=clang cman build
```

Every project is compiled with `-Wall -Wextra`. A build tool that hides what the compiler
has to say is not doing its job.

## Not included

Deliberately out of scope for now: dependency resolution and a package registry, libraries
(only executables are produced), a test runner, custom build profiles, and non-Unix
toolchains. The manifest has room to grow into these; nothing above depends on their
absence.

## Development

```console
$ cargo test              # unit tests for the .d parser and package-name validation
$ ./tests/e2e.sh          # 50 assertions against real C projects
$ cargo clippy --all-targets
```

`tests/e2e.sh` is where the behaviour that matters is actually verified. It builds real
projects in a temporary directory and checks the things unit tests cannot see: that a
touched header rebuilds its dependents and nothing else, that `--release` and debug stay
independent, that a failed compile leaves no object behind, that exit codes propagate, and
that 25 units compiled in parallel still link correctly. Point it at any binary with
`CMAN=$(command -v cman) ./tests/e2e.sh`.

## License

MIT
