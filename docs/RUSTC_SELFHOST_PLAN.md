# Plan: Official rustc Self-Hosting Inside sysrust

## The Goal

Official rustc runs inside sysrust. Compiles itself. That copy compiles itself. The system is fully self-contained — no external tools needed after initial bootstrap.

## Critical Issue Found in Previous Draft

**lld is C++.** The previous plan said "rustc compiles lld from source" — but rustc can't compile C++. For true self-containment, we need a **Rust-based linker**, not lld. This is the key insight that changes the approach.

## Revised Self-Contained End State

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

After bootstrap: sysrust contains a Rust compiler + linker, both written in Rust, both compiled by themselves. New code can be written and compiled entirely within the OS.

## Why Cranelift Backend (Not LLVM)

- **Cranelift is pure Rust** — no C++ dependency, compiled by rustc as a regular crate
- **LLVM is 20M lines of C++** — can't be compiled inside sysrust without a C++ compiler
- **Cranelift supports x86_64** — confirmed, our OS is x86_64
- **Cranelift is PART OF rustc** — it lives in `compiler/rustc_codegen_cranelift/`. When rustc compiles itself, it compiles cranelift too. Cranelift self-compilation comes for free.
- **The self-hosted rustc uses cranelift to compile itself** — full Rust loop, including the code generator

## The Rust-Based Linker (~3-5K lines)

Instead of lld (C++), we build `sysrust-link` — a minimal static ELF linker in Rust:

- Reads `.o` files (ELF relocatable objects, which cranelift produces)
- Resolves symbols across object files
- Applies x86_64 relocations (R_X86_64_PC32, R_X86_64_PLT32, R_X86_64_64, etc.)
- Writes the final ELF64 executable with proper headers and sections
- No dynamic linking, no LTO, no debug info optimization — just enough for self-hosting

This is a well-scoped project: ~3,000-5,000 lines. It only needs to handle what `rustc --codegen-backend=cranelift` produces. The `object` crate (pure Rust, no_std compatible) handles ELF parsing.

## Key Technical Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Compiler | Official rustc | Reusing 500K lines of tested code vs building from scratch |
| Backend | Cranelift | Pure Rust, no C++, supports x86_64 |
| Linker | New Rust linker | lld is C++ — can't self-host without C++ compiler |
| std impl | Fork + PAL | ~2-3K lines mapping std to our syscalls |
| Memory | 4GB QEMU | rustc needs 1.5-4GB; trivial to extend PMM |
| Filesystem | VFS with dirs | rustc needs deep directory structures |
| Process spawn | Not needed | Two-step: rustc --emit obj, then sysrust-link |

## Potential Problems Identified

### 1. HashMap needs randomness
Every HashMap in rustc uses `RandomState` which calls `getrandom`. Our OS has no entropy source.
**Fix:** Implement `getrandom` syscall returning timer tick counter. Not cryptographic but sufficient for HashMap seed.

### 2. Cranelift backend is "experimental"
The cranelift backend may not handle all of rustc's codegen requirements.
**Fix:** Test early (Phase 3). If cranelift can't compile rustc, fall back to LLVM backend — but then lld stays cross-compiled and the system isn't fully self-contained for the linker.

### 3. Getting source into the OS
rustc source is ~500K lines across ~3000 files in deep directories.
**Fix:** Build a tar archive on the host, include it on disk image, unpack to ramfs. Or extend FAT to support subdirectories.

### 4. rustc build system complexity
`x.py` / bootstrap is complex. Cross-compilation to a new target may have unexpected issues.
**Fix:** Follow Redox OS and Fuchsia as reference implementations. Both added custom targets to the Rust build system.

### 5. Process::Command returns error
rustc tries to spawn the linker. Our std PAL returns an error for Command.
**Fix:** Configure rustc to emit object files only. The shell script / wrapper handles the link step. Or: patch rustc's linker invocation to call our sysrust-link as a library function.

## Phased Implementation

### Phase 1: OS Infrastructure (~1 session)
**Goal: sysrust can handle compilation workloads**

- Extend PMM/VMM for 4GB RAM (~50 lines)
- VFS directory support + mkdir + unlink + rename (~500 lines)
- Additional syscalls: lseek, real fstat, getcwd, mkdir, getdents64, getrandom (~300 lines)
- Proper allocator in sysrust-rt: free-list instead of bump (~200 lines)

### Phase 2: Fork rust-lang/rust + std PAL (~1-2 sessions)
**Goal: rustc can be cross-compiled for sysrust**

- Fork rust-lang/rust
- Add `x86_64-unknown-sysrust` target spec (~100 lines)
- Implement `library/std/src/sys/pal/sysrust/` (~2,500 lines):
  - File I/O (open/read/write/close/lseek/stat)
  - stdio (fd 0/1/2 wrappers)
  - alloc (brk-based)
  - args/env (minimal stubs)
  - TLS (arch_prctl FS base)
  - getrandom (timer-based)
  - process::exit
  - thread (single-threaded stub)
  - time (stub)
- Build std for sysrust: `./x.py build --target x86_64-unknown-sysrust library`

### Phase 3: Cross-Compile rustc (~1 session)
**Goal: a rustc binary that runs inside sysrust**

- `./x.py build --target x86_64-unknown-sysrust --stage 1 compiler/rustc`
- Use cranelift backend: `codegen-backends = ["cranelift"]`
- Strip binary, measure size (~50-100MB?)
- Package on disk image with std libraries (.rlib files)

### Phase 4: First Compilation (~iterative, multiple sessions)
**Goal: rustc runs inside sysrust and compiles hello.rs**

- Load rustc + std libs from disk
- `run rustc --edition 2024 --emit obj -o hello.o hello.rs`
- Debug missing syscalls iteratively (expect 10-20 new stubs)
- Each missing syscall: implement or stub, rebuild kernel, retry

### Phase 5: Build sysrust-link (~1-2 sessions)
**Goal: a Rust-based ELF linker running inside sysrust**

- Read ELF relocatable objects (.o files)
- Symbol table merging and resolution
- x86_64 relocation application (~15 relocation types)
- ELF64 executable output with proper headers
- ~3,000-5,000 lines of Rust
- Cross-compile for sysrust, test inside OS

### Phase 6: Self-Hosting Bootstrap (~iterative)
**Goal: rustc + sysrust-link compile themselves inside sysrust**

- Package rustc source + sysrust-link source on disk
- Inside sysrust: compile rustc source with rustc
- Inside sysrust: compile sysrust-link source with rustc
- Run the newly compiled rustc + sysrust-link to compile again
- Verify bit-for-bit identical output

## What We're NOT Building

- A Unix (no fork, exec, signals, pipes, users, permissions)
- A libc (std talks directly to syscalls)
- A custom compiler (using official rustc)
- A C++ toolchain (linker is Rust, backend is cranelift)
- Full Linux compatibility (just enough for rustc)

## Estimated Total Effort

| Phase | New code | Sessions |
|-------|---------|----------|
| Phase 1: OS infra | ~1,050 lines | 1 |
| Phase 2: Rust fork + PAL | ~2,600 lines | 1-2 |
| Phase 3: Cross-compile | Config/scripts | 1 |
| Phase 4: First compilation | Stubs/fixes | 2-4 |
| Phase 5: Rust linker | ~4,000 lines | 1-2 |
| Phase 6: Self-hosting | Testing/fixes | 2-4 |
| **Total** | **~7,650 lines** | **8-14 sessions** |

## Files Modified/Created in sysrust

| File | Changes |
|------|---------|
| `src/pmm.rs` | Extend bitmap to 4GB |
| `boot/boot.s` | Identity-map 4GB |
| `src/vfs.rs` | Directory support |
| `src/ramfs.rs` | Directory nodes, temp files |
| `src/syscall.rs` | lseek, fstat, mkdir, getdents64, getrandom, rename |
| `sysrust-rt/src/allocator.rs` | Free-list allocator |
| `sysrust-rt/src/sys.rs` | New syscall wrappers |
| `Makefile` | -m 4096 for QEMU |
| **New: `sysrust-link/`** | Rust-based ELF linker crate |
