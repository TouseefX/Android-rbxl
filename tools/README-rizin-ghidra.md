# rizin + rz-ghidra (pdg) — Linux x86-64

`rizin-ghidra-v0.9.1-x86_64.7z.001` is rizin 0.9.1 built as **shared** libraries
with the rz-ghidra v0.9.0 plugin, i.e. the `pdg` Ghidra decompiler and the Sleigh
disassembler. It exists because the previously committed
`rizin-v0.9.1-static-x86_64.7z.*` cannot load a decompiler at all: it is fully
static, has no dynamic section, and `nm -D --defined-only` on it reports **0**
exported symbols, so a plugin `.so` has nothing to link against.

## Use

```sh
7zz x -o~/.local/rizin-ghidra tools/rizin-ghidra-v0.9.1-x86_64.7z.001
~/.local/rizin-ghidra/bin/rizin -qc "aa; s entry0; af; pdg" some.exe
```

No `LD_LIBRARY_PATH` is needed. The binaries carry
`RUNPATH=$ORIGIN/../lib/x86_64-linux-gnu`, so the tree is relocatable — verified
by extracting to a fresh prefix and decompiling from there.

Useful commands: `pdg` (pseudo-C for the current function), `pdgj` (JSON),
`pdgs` (loaded Sleigh languages), `pdg*` (attach the output as a comment).

## Reproducing from source

`gcc`/`g++` 12.2 is enough; `cmake`, `meson`, `ninja` and `pkgconf` come from
PyPI (`python3 -m venv` then `pip install cmake meson ninja pkgconf`). flex and
bison are **not** required — rz-ghidra ships pre-generated `slghparse.cc` /
`slghscan.cc` and `GENERATE_PARSERS` defaults to `OFF`.

Four things need working around, all of them network-related:

1. **pcre2, zlib, zstd, libzip** — the wrap files point at release-asset URLs that
   serve a *dist* tarball, which is unreachable from a restricted network and
   whose hash differs from the git tarball anyway. Fetch the git tarball from
   `codeload.github.com/<owner>/<repo>/tar.gz/refs/tags/<tag>`, repack it so the
   top-level directory is exactly the wrap's `directory =` value (codeload
   produces `<repo>-<tag>`, which silently extracts to nothing useful), drop it in
   `subprojects/packagecache/`, and rewrite `source_hash`. rizin supplies its own
   `meson.build` for each under `subprojects/packagefiles/`, so the git tree
   builds fine.
2. **pcre2's `deps/sljit`** is a submodule and is absent from the git tarball, so
   `pcre2_jit_compile.c` fails on a missing `sljitLir.c`. Clone
   `zherczeg/sljit` into `subprojects/pcre2-10.47/deps/sljit`.
3. **zydis** — the wrap wants `zydis-amalgamated.tar.gz`, a generated release
   asset. It is not actually needed: rizin's own
   `subprojects/packagefiles/zydis/amalgamated-dist/Zydis.{c,h}` is what its
   `meson.build` compiles. Any small placeholder tarball with a matching
   `source_hash` works.
4. **rz-ghidra needs a system zlib**, which rizin's subproject build does not
   install. Build `zlib-1.3.2` separately with `--prefix` pointing at the same
   install prefix before configuring rz-ghidra.

Then:

```sh
# rizin
git clone --depth 1 --branch v0.9.1 --recursive https://github.com/rizinorg/rizin
cd rizin && meson setup build --prefix="$PREFIX" --default-library=shared \
    --buildtype=release -Duse_lzma=false -Drizin_sigdb=false -Ddebugger=false \
    -Denable_tests=false -Denable_benchmarks=false -Drizin_bindings=false
ninja -C build && ninja -C build install

# rz-ghidra
git clone --depth 1 --branch v0.9.0 --recursive https://github.com/rizinorg/rz-ghidra
cd rz-ghidra && mkdir build && cd build
cmake -DCMAKE_INSTALL_PREFIX="$PREFIX" -DCMAKE_PREFIX_PATH="$PREFIX" \
      -DCMAKE_POLICY_VERSION_MINIMUM=3.5 -DCMAKE_BUILD_TYPE=Release ..
cmake --build . -j"$(nproc)" && cmake --install .
```

`CMAKE_POLICY_VERSION_MINIMUM=3.5` is needed because rz-ghidra declares
`cmake_minimum_required(VERSION 3.0...3.5)` and CMake 4 rejects a lower bound
below 3.5. rizin is found through the `RizinConfig.cmake` it installs, not
pkg-config — there is no `rizin.pc`, only per-module `rz_*.pc` files.

Binaries in the archive are stripped, so `main` and friends are not named
symbols; decompile by address (`s entry0; af; pdg`) or run `aa`/`aaa` first.
