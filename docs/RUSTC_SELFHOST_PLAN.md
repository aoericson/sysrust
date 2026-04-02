# Plan: Official rustc Self-Hosting Inside sysrust

**Last updated: 2026-04-02**

## The Goal

Official rustc runs inside sysrust. Compiles itself. That copy compiles itself. The system is fully self-contained — no external tools needed after initial bootstrap.

## Current Progress

| Phase | Status | Details |
|-------|--------|---------|
| Phase 1: OS Infrastructure | **DONE** | 4GB RAM, VFS directories, 9 syscalls, free-list allocator |
| Phase 2: Fork rust + std PAL | **IN PROGRESS** | Target spec + PAL files written, build blocked by version mismatch |
| Phase 3: Cross-compile rustc | Pending | |
| Phase 4: First compilation | Pending | |
| Phase 5: Rust-based linker | Pending | |
| Phase 6: Self-hosting | Pending | |

### Phase 1 Completed (2026-04-02)

- PMM bitmap extended to 1M pages (4GB support)
- Boot assembly identity-maps 4GB with 2MB pages (4 page directories)
- QEMU runs with -m 4096 (3070MB free detected)
- VFS directory support: mkdir, resolve_path, mkdir_p, unlink, rename
- Ramfs directory nodes with readdir/finddir
- 9 new syscalls: lseek, fstat, getcwd, mkdir, getdents64, getrandom, rename, unlink, unlinkat
- Free-list allocator in sysrust-rt (replaces bump allocator, supports dealloc)
- All existing tests pass

### Phase 2 Started (2026-04-02)

Rust fork at `~/src/rust-sysrust/` (shallow clone of rust-lang/rust HEAD).

Files created in the fork:
- `compiler/rustc_target/src/spec/targets/x86_64_unknown_sysrust.rs` — target spec
- `library/std/src/sys/pal/sysrust/mod.rs` — PAL (delegates to unsupported baseline)
- `library/std/src/sys/alloc/sysrust.rs` — brk-based allocator
- `library/std/src/os/sysrust/mod.rs` — OS module (empty)
- Plus edits to: pal/mod.rs, alloc/mod.rs, random/mod.rs, env_consts.rs, exit.rs, os/mod.rs, spec/mod.rs

**BLOCKED**: The stage0 compiler (nightly-2026-03-05) cannot compile HEAD source due to `target_spec_enum!` macro changes. Need to re-clone at a release tag where source matches stage0.

### Next Step

1. Delete current shallow clone, re-clone at release tag `1.86.0` (or matching nightly)
2. Re-apply the 11-file sysrust target patch
3. `BOOTSTRAP_SKIP_TARGET_SANITY=1 ./x.py build library --target x86_64-unknown-sysrust`
4. When std builds, cross-compile rustc itself

## Architecture

### Self-Contained End State

```
Bootstrap (once, on host):
  Host rustc cross-compiles:
    - rustc (cranelift backend) → sysrust-rustc-stage0
    - sysrust-link (our Rust linker) → sysrust-link-stage0

Inside sysrust (fully self-contained):
  sysrust-rustc-stage0 + sysrust-link-stage0 compile:
    - rustc source → sysrust-rustc-stage1
    - sysrust-link source → sysrust-link-stage1
  sysrust-rustc-stage1 + sysrust-link-stage1 compile:
    - rustc source → sysrust-rustc-stage2
    - sysrust-link source → sysrust-link-stage2
  Verify: stage1 == stage2 (bit-for-bit identical)
```

### Why Cranelift (Not LLVM)

- Cranelift is pure Rust (~200K lines, no C++)
- Cranelift is PART OF rustc (compiled as a regular crate during self-hosting)
- LLVM is 20M lines of C++ — can't self-host without a C++ compiler
- Cranelift supports x86_64

### Why a Rust-Based Linker (Not lld)

- lld is C++ — can't self-host without a C++ compiler
- `sysrust-link` is a minimal ELF linker in Rust (~3-5K lines)
- Only needs to handle what cranelift produces (no LTO, no dynamic linking)
- The `object` crate (pure Rust, no_std) handles ELF parsing

### Key Technical Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Compiler | Official rustc | Reusing 500K lines of tested code |
| Backend | Cranelift | Pure Rust, no C++, supports x86_64 |
| Linker | New Rust linker | lld is C++ |
| std impl | Fork + PAL | ~2-3K lines mapping std to our syscalls |
| Memory | 4GB QEMU | rustc needs 1.5-4GB |
| Filesystem | VFS with dirs | rustc needs deep directory structures |
| Process spawn | Not needed | Two-step: rustc --emit obj, then sysrust-link |

## What We're NOT Building

- A Unix (no fork, exec, signals, pipes, users, permissions)
- A libc (std talks directly to syscalls)
- A custom compiler (using official rustc)
- A C++ toolchain (linker is Rust, backend is cranelift)
- Full Linux compatibility (just enough for rustc)

## OS Capabilities (as of Phase 1)

| Capability | Status |
|-----------|--------|
| x86_64 long mode | ✅ 4-level paging, 4GB identity map |
| ELF64 loader | ✅ Loads cross-compiled Rust binaries |
| Linux syscall ABI | ✅ syscall instruction + int 0x80 |
| File I/O | ✅ open, read, write, close, lseek, fstat |
| Directories | ✅ mkdir, resolve_path, readdir, unlink, rename |
| Memory | ✅ mmap, brk, 4GB RAM |
| Heap allocator | ✅ Free-list with dealloc (in sysrust-rt) |
| TLS | ✅ arch_prctl(ARCH_SET_FS) |
| Random | ✅ getrandom (timer-based, for HashMap) |
| Network | ✅ Ethernet, ARP, IPv4, ICMP, UDP, TCP, DNS |
| Disk | ✅ ATA PIO, FAT16 |
