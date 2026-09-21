#!/usr/bin/env bash
#
# End-to-end test suite for cman.
#
# Drives the real binary against real C projects in a temporary directory,
# which is where the interesting behaviour lives: incremental rebuilds,
# dependency tracking and exit codes cannot be observed from unit tests.
#
# Usage:
#   tests/e2e.sh                    # builds the debug binary and tests it
#   CMAN=/path/to/cman tests/e2e.sh # tests an already-installed binary
#
# Exits non-zero if any assertion fails.

set -u

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

if [ -z "${CMAN:-}" ]; then
    cargo build --manifest-path "$repo_root/Cargo.toml" >/dev/null || exit 1
    CMAN="$repo_root/target/debug/cman"
fi

if [ ! -x "$CMAN" ]; then
    echo "no cman binary at '$CMAN'" >&2
    exit 1
fi

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
cd "$WORK" || exit 1

pass=0
fail=0

# check <description> <expected-substring> <actual>
check() {
    if grep -qF -- "$2" <<<"$3"; then
        echo "  PASS  $1"
        pass=$((pass + 1))
    else
        echo "  FAIL  $1"
        echo "        expected to contain: $2"
        echo "        got: $3"
        fail=$((fail + 1))
    fi
}

# refute <description> <forbidden-substring> <actual>
refute() {
    if grep -qF -- "$2" <<<"$3"; then
        echo "  FAIL  $1"
        echo "        expected NOT to contain: $2"
        echo "        got: $3"
        fail=$((fail + 1))
    else
        echo "  PASS  $1"
        pass=$((pass + 1))
    fi
}

echo "testing: $CMAN"
echo "workdir: $WORK"

echo
echo "=== 1. new ==="
out=$("$CMAN" new hello 2>&1)
check "new reports creation" "Created" "$out"
check "manifest exists" "cman.toml" "$(ls hello)"
cd hello || exit 1

echo
echo "=== 2. build (cold) ==="
out=$("$CMAN" build 2>&1)
check "cold build compiles" "Compiling hello v0.1.0" "$out"
check "binary produced" "hello" "$(ls target/debug)"
check "dep file produced" "main.d" "$(ls target/debug/obj)"

echo
echo "=== 3. build (warm, nothing changed) ==="
out=$("$CMAN" build 2>&1)
refute "warm build does no work" "Compiling" "$out"

echo
echo "=== 4. run ==="
out=$("$CMAN" run 2>&1)
check "run executes the binary" "Hello, world!" "$out"

echo
echo "=== 5. multi-file project with a shared header ==="
mkdir -p include src/math
cat > include/greet.h <<'EOF'
#ifndef GREET_H
#define GREET_H
void greet(const char *who);
#endif
EOF
cat > src/greet.c <<'EOF'
#include <stdio.h>
#include "greet.h"

void greet(const char *who) { printf("Hello, %s!\n", who); }
EOF
cat > src/math/add.h <<'EOF'
#ifndef ADD_H
#define ADD_H
int add(int a, int b);
#endif
EOF
cat > src/math/add.c <<'EOF'
#include "math/add.h"

int add(int a, int b) { return a + b; }
EOF
cat > src/main.c <<'EOF'
#include <stdio.h>
#include "greet.h"
#include "math/add.h"

int main(int argc, char **argv) {
    greet(argc > 1 ? argv[1] : "world");
    printf("2 + 3 = %d\n", add(2, 3));
    return 0;
}
EOF
out=$("$CMAN" build 2>&1)
check "multi-file build succeeds" "Finished" "$out"
check "nested object mirrors the src tree" "add.o" "$(ls target/debug/obj/math)"
out=$("$CMAN" run -- Michael 2>&1)
check "run forwards arguments" "Hello, Michael!" "$out"
check "nested module is linked in" "2 + 3 = 5" "$out"

echo
echo "=== 6. incremental: a touched header rebuilds only its dependents ==="
before_main=$(stat -c %Y target/debug/obj/main.o)
before_add=$(stat -c %Y target/debug/obj/math/add.o)
sleep 1.1
touch include/greet.h
"$CMAN" build >/dev/null 2>&1
after_main=$(stat -c %Y target/debug/obj/main.o)
after_add=$(stat -c %Y target/debug/obj/math/add.o)
[ "$before_main" != "$after_main" ] && r="rebuilt" || r="skipped"
check "main.o rebuilt (it includes greet.h)" "rebuilt" "$r"
[ "$before_add" != "$after_add" ] && r="rebuilt" || r="skipped"
check "add.o skipped (it does not include greet.h)" "skipped" "$r"

echo
echo "=== 7. incremental: a manifest change invalidates the fingerprint ==="
out=$("$CMAN" build 2>&1)
refute "still warm before the manifest edit" "Compiling" "$out"
sed -i 's/std = "c17"/std = "c11"/' cman.toml
out=$("$CMAN" build 2>&1)
check "a flag change forces a rebuild" "Compiling" "$out"
sed -i 's/std = "c11"/std = "c17"/' cman.toml
"$CMAN" build >/dev/null 2>&1

echo
echo "=== 8. profiles are separate trees ==="
out=$("$CMAN" build --release 2>&1)
check "release build runs" "optimized" "$out"
check "release binary exists" "hello" "$(ls target/release)"
out=$("$CMAN" build 2>&1)
refute "the debug tree is untouched by a release build" "Compiling" "$out"

echo
echo "=== 9. run --release executes the release binary ==="
cp src/main.c "$WORK/main.c.bak"
cat > src/main.c <<'EOF'
#include <stdio.h>

int main(void) {
#ifdef NDEBUG
    puts("RELEASE");
#else
    puts("DEBUG");
#endif
    return 0;
}
EOF
out=$("$CMAN" run 2>&1)
check "run uses the debug binary" "DEBUG" "$out"
check "run reports the debug path" "target/debug/hello" "$out"
out=$("$CMAN" run --release 2>&1)
# NDEBUG is only defined by the release profile, so the program's own output
# proves which of the two binaries actually ran.
check "run --release uses the release binary" "RELEASE" "$out"
check "run --release reports the release path" "target/release/hello" "$out"
out=$("$CMAN" run -- --release 2>&1)
check "a flag after -- goes to the program, not to cman" "DEBUG" "$out"
cp "$WORK/main.c.bak" src/main.c
"$CMAN" build >/dev/null 2>&1

echo
echo "=== 10. check ==="
"$CMAN" clean >/dev/null 2>&1
out=$("$CMAN" check 2>&1)
check "check reports checking" "Checking hello" "$out"
[ -e target/debug/obj ] && o="objects written" || o="no objects"
check "check produces no object code at all" "no objects" "$o"
"$CMAN" build >/dev/null 2>&1

echo
echo "=== 11. errors surface with a non-zero exit code ==="
cp src/main.c "$WORK/main.c.bak"
# A genuine parse error: many type mistakes are only warnings in older GCC.
echo "int broken(void) { return }" >> src/main.c
out=$("$CMAN" check 2>&1); code=$?
check "the compiler diagnostic is forwarded" "src/main.c" "$out"
check "the failure is reported" "could not check" "$out"
[ "$code" -ne 0 ] && e="nonzero" || e="zero"
check "check exits non-zero on error" "nonzero" "$e"
out=$("$CMAN" build 2>&1); code=$?
check "build names the failing file" "could not compile" "$out"
[ "$code" -ne 0 ] && e="nonzero" || e="zero"
check "build exits non-zero on error" "nonzero" "$e"
[ -e target/debug/obj/main.o ] && o="present" || o="absent"
check "no object is left behind by a failed compile" "absent" "$o"
cp "$WORK/main.c.bak" src/main.c
out=$("$CMAN" build 2>&1)
check "the next build recovers" "Finished" "$out"

echo
echo "=== 12. removing a source file relinks without its stale object ==="
cat > src/main.c <<'EOF'
#include <stdio.h>
#include "math/add.h"

int main(void) {
    printf("2 + 3 = %d\n", add(2, 3));
    return 0;
}
EOF
rm src/greet.c include/greet.h
out=$("$CMAN" build 2>&1)
check "build succeeds after a source is removed" "Finished" "$out"
out=$("$CMAN" run 2>&1)
check "the rest of the program still runs" "2 + 3 = 5" "$out"

echo
echo "=== 13. run forwards the program's exit code ==="
cat > src/main.c <<'EOF'
int main(void) { return 42; }
EOF
rm -rf src/math
"$CMAN" clean >/dev/null 2>&1
"$CMAN" run >/dev/null 2>&1; code=$?
[ "$code" -eq 42 ] && e="42" || e="$code"
check "the exit code survives" "42" "$e"

echo
echo "=== 14. the project is discovered from a subdirectory ==="
mkdir -p deep/nested && cd deep/nested || exit 1
out=$("$CMAN" build 2>&1)
check "finds the project root by walking up" "Finished" "$out"
cd "$WORK/hello" || exit 1

echo
echo "=== 15. clean ==="
"$CMAN" build --release >/dev/null 2>&1
out=$("$CMAN" clean --release 2>&1)
check "clean --release removes files" "file(s) from" "$out"
check "the debug tree survives" "debug" "$(ls target)"
refute "the release tree is gone" "release" "$(ls target)"
"$CMAN" clean >/dev/null 2>&1
[ -d target ] && t="present" || t="gone"
check "clean removes the whole target dir" "gone" "$t"

echo
echo "=== 16. a missing project is reported clearly ==="
cd "$WORK" || exit 1
out=$("$CMAN" build 2>&1); code=$?
check "the missing manifest is named" "could not find \`cman.toml\`" "$out"
[ "$code" -ne 0 ] && e="nonzero" || e="zero"
check "exits non-zero without a project" "nonzero" "$e"

echo
echo "=== 17. init preserves an existing directory ==="
mkdir -p existing/src
cat > existing/src/main.c <<'EOF'
#include <stdio.h>
int main(void) { printf("preserved\n"); return 0; }
EOF
cd existing || exit 1
out=$("$CMAN" init 2>&1)
check "init scaffolds in place" "Created" "$out"
check "init keeps existing sources" "preserved" "$(cat src/main.c)"
out=$("$CMAN" run 2>&1)
check "the preserved source is what runs" "preserved" "$out"
out=$("$CMAN" init 2>&1)
check "init refuses to clobber a project" "already exists" "$out"

echo
echo "=== 18. invalid package names are rejected ==="
cd "$WORK" || exit 1
out=$("$CMAN" new "bad name" 2>&1); code=$?
check "a name with a space is rejected" "invalid package name" "$out"
[ "$code" -ne 0 ] && e="nonzero" || e="zero"
check "exits non-zero on an invalid name" "nonzero" "$e"

echo
echo "=== 19. parallel compilation stays correct ==="
"$CMAN" new big >/dev/null 2>&1
cd big || exit 1
for i in $(seq 1 24); do
    printf 'int f%d(void) { return %d; }\n' "$i" "$i" > "src/f$i.c"
done
{
    echo "int main(void) { int s = 0;"
    for i in $(seq 1 24); do echo "extern int f$i(void); s += f$i();"; done
    echo "return s == 300 ? 0 : 1; }"
} > src/main.c
"$CMAN" build >/dev/null 2>&1
check "every unit is compiled" "25" "$(ls target/debug/obj/*.o | wc -l)"
"$CMAN" run >/dev/null 2>&1; code=$?
[ "$code" -eq 0 ] && e="linked" || e="wrong"
check "all 24 units link and produce the right answer" "linked" "$e"

echo
echo "=== 20. VS Code auto-save settings ==="
cd "$WORK" || exit 1
check "new writes the settings file" "onFocusChange" "$(cat big/.vscode/settings.json)"
# A project that predates the feature, or one whose .vscode was deleted: the
# build command has to put it back on its own.
rm -rf big/.vscode
cd big || exit 1
out=$("$CMAN" build 2>&1)
check "build reports configuring the editor" "Configured" "$out"
check "build writes the settings file" "onFocusChange" "$(cat .vscode/settings.json)"
out=$("$CMAN" build 2>&1)
refute "a second build says nothing about it" "Configured" "$out"
# JSONC that no strict JSON parser would accept: proof that cman rewrites
# nothing rather than merging into settings it cannot safely parse.
printf '{\n  // mine\n  "files.autoSave": "off",\n}\n' > .vscode/settings.json
"$CMAN" clean >/dev/null 2>&1
"$CMAN" check >/dev/null 2>&1
check "existing settings are left alone" '"files.autoSave": "off"' "$(cat .vscode/settings.json)"
check "even the comments survive" "// mine" "$(cat .vscode/settings.json)"
rm -rf .vscode
CMAN_NO_EDITOR_SETUP=1 "$CMAN" build >/dev/null 2>&1
[ -e .vscode ] && v="written" || v="absent"
check "CMAN_NO_EDITOR_SETUP turns the feature off" "absent" "$v"
cd "$WORK" || exit 1
out=$(CMAN_NO_EDITOR_SETUP=1 "$CMAN" new quiet 2>&1)
refute "the opt-out covers new as well" "Configured" "$out"

echo
echo "======================================"
echo "  passed: $pass   failed: $fail"
echo "======================================"
[ "$fail" -eq 0 ]
