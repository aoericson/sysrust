# opsys / sysrust Feature Matrix

Last updated: 2026-04-02

## Status Legend

| Symbol | Meaning |
|--------|---------|
| OK     | Implemented and working |
| BUG    | Implemented but has known bugs |
| SKIP   | Intentionally skipped for this project |
| TODO   | Planned but not started |
| WIP    | Work in progress |
| N/A    | Not applicable to this language/project |

## Feature Matrix

### Boot and CPU

| Feature                  | opsys (C) | sysrust (Rust) | Notes |
|--------------------------|-----------|----------------|-------|
| Multiboot boot           | OK        | OK             | |
| 16KB boot stack          | OK        | OK             | |
| GDT (3-entry flat)       | OK        | OK             | |
| IDT (256 entries)        | OK        | OK             | |
| ISR/IRQ stubs (NASM)     | OK        | OK             | Identical assembly |
| PIC remap (8259)         | OK        | OK             | |
| Exception handlers       | OK        | OK             | |

### Memory Management

| Feature                  | opsys (C) | sysrust (Rust) | Notes |
|--------------------------|-----------|----------------|-------|
| PMM (bitmap allocator)   | OK        | OK             | |
| VMM (x86 paging)         | OK        | OK             | |
| Heap (free-list)         | OK        | OK             | |
| Identity map (16MB)      | OK        | OK             | |

### Multitasking and Sync

| Feature                  | opsys (C) | sysrust (Rust) | Notes |
|--------------------------|-----------|----------------|-------|
| Preemptive threads (32)  | OK        | OK             | |
| Round-robin scheduler    | OK        | OK             | |
| Context switch (asm)     | OK        | OK             | |
| Spinlock (cli/sti)       | OK        | OK             | |
| Mutex (yield-based)      | OK        | OK             | |

### Hardware Drivers

| Feature                  | opsys (C) | sysrust (Rust) | Notes |
|--------------------------|-----------|----------------|-------|
| VGA text (80x25)         | OK        | OK             | |
| VGA 80x50 mode           | OK        | OK             | |
| VGA-to-serial mirror     | OK        | OK             | |
| PS/2 keyboard            | OK        | OK             | |
| PIT timer (100Hz)        | OK        | OK             | |
| PCI bus enumeration      | OK        | OK             | |
| RTL8139 NIC driver       | OK        | OK             | |
| ATA PIO disk driver      | OK        | OK             | |
| Serial (COM1 TX+RX)      | OK        | OK             | |

### Network Stack

| Feature                  | opsys (C) | sysrust (Rust) | Notes |
|--------------------------|-----------|----------------|-------|
| Ethernet framing         | OK        | OK             | |
| ARP (resolve + table)    | OK        | OK             | |
| IPv4 (send/recv)         | OK        | OK             | |
| ICMP (ping)              | OK        | OK             | |
| UDP                      | OK        | OK             | |
| DNS resolver             | OK        | OK             | |
| TCP client               | OK        | OK             | |
| TCP server               | TODO      | TODO           | |

### Filesystem

| Feature                  | opsys (C) | sysrust (Rust) | Notes |
|--------------------------|-----------|----------------|-------|
| Initrd (custom format)   | OK        | OK             | |
| RAM filesystem           | OK        | OK             | |
| VFS layer                | OK        | OK             | |
| Device nodes (/dev/*)    | OK        | OK             | |
| FAT16 filesystem         | OK        | OK             | |
| FAT16 VFS integration    | OK        | OK             | |

### Shell

| Feature                  | opsys (C) | sysrust (Rust) | Notes |
|--------------------------|-----------|----------------|-------|
| Interactive REPL         | OK        | OK             | |
| help, clear, echo        | OK        | OK             | |
| ls, cat, touch, write    | OK        | OK             | |
| edit (text editor)       | OK        | OK             | |
| cc (compile), run        | OK        | OK             | |
| ping, resolve            | OK        | OK             | |
| tcp, http                | OK        | OK             | |
| save, load (FAT16)       | OK        | OK             | |
| mem, threads, spawn      | OK        | OK             | |
| mirror, reboot           | OK        | OK             | |

### Built-in Compiler

Each OS has its own compiler matching its language. opsys compiles C; sysrust compiles a Rust subset.

#### opsys: C Compiler

| Feature                  | opsys (C) | Notes |
|--------------------------|-----------|-------|
| Lexer (67 tokens)        | OK        | |
| Parser (recursive desc)  | OK        | |
| x86 emitter              | OK        | |
| Symbol table (31 builtins)| OK       | |
| Types: int, char, void, ptr | OK     | |
| Control: if/else/while/for | OK      | |
| Operators (full set)     | OK        | |
| Strings + #define        | OK        | |
| Structs                  | OK        | |
| Ternary + do-while       | OK        | |
| Enum support             | OK        | |
| Switch/case              | OK        | |
| #include (dedup)         | OK        | |
| typedef                  | OK        | |
| Function prototypes      | OK        | |
| static/const/extern kw   | OK        | |
| Array fields in structs  | OK        | |
| unsigned/signed/short/long| OK       | |

#### sysrust: Rust-Subset Compiler (rc)

| Feature                  | sysrust   | Notes |
|--------------------------|-----------|-------|
| Lexer (Rust tokens)      | TODO      | fn, let, mut, struct, if/else, while, for, loop |
| Parser (recursive desc)  | TODO      | Single-pass codegen, Rust syntax |
| x86 emitter              | OK        | Shared architecture with C compiler |
| Symbol table             | TODO      | Rust-style: fn, let, struct |
| Types: i32, u8, u32, bool| TODO      | No lifetimes, no generics |
| Pointers: *const/*mut    | TODO      | Raw pointers only |
| Control: if/while/for/loop| TODO     | No match (initially) |
| let/let mut bindings     | TODO      | Type inference not needed (explicit types) |
| fn with -> return type   | TODO      | |
| Structs (no impl)        | TODO      | Value types, field access |
| Arrays: [T; N]           | TODO      | Fixed-size |
| Operators (full set)     | TODO      | Same as C compiler |
| String literals (b"...")  | TODO      | Byte strings |
| Kernel builtins          | TODO      | puts, putchar, malloc, etc. |
| Test programs (.rs)      | TODO      | Rewrite 23 C programs in Rust subset |

### Build Tools

| Feature                  | opsys (C) | sysrust (Rust) | Notes |
|--------------------------|-----------|----------------|-------|
| mkinitrd (host tool)     | C binary  | Rust binary    | `rustc -o tools/mkinitrd tools/mkinitrd.rs` |
| Test programs language   | .c files  | .rs files      | 21 Rust-syntax programs, 15/15 autotest pass |

### x86_64 / ELF (sysrust only)

| Feature                  | opsys (C) | sysrust (Rust) | Notes |
|--------------------------|-----------|----------------|-------|
| Architecture             | i686 (32-bit) | x86_64 (64-bit) | sysrust ported to long mode |
| Paging                   | 2-level   | 4-level (PML4) | sysrust: 4GB identity map |
| ELF loader               | N/A       | OK (ELF32+ELF64) | Loads cross-compiled Rust binaries |
| Linux syscall (syscall instr) | N/A  | OK             | x86_64 ABI, 40+ syscalls |
| mmap                     | N/A       | OK             | Anonymous memory allocation |
| VFS directories          | N/A       | OK             | mkdir, resolve_path, unlink, rename |
| Free-list allocator (rt) | N/A       | OK             | sysrust-rt with dealloc support |
| 4GB RAM                  | N/A       | OK             | QEMU -m 4096, 3GB usable |
| TLS (arch_prctl)         | N/A       | OK             | FS segment base for thread-local storage |
| getrandom                | N/A       | OK             | Timer-based PRNG for HashMap seeds |
| Real Rust programs       | N/A       | OK             | Vec, String, format!, closures, iterators, Box |

### Crash Recovery

| Feature                  | opsys (C) | sysrust (Rust) | Notes |
|--------------------------|-----------|----------------|-------|
| Fault handler -> shell   | OK        | OK             | |
| Protected execution      | OK        | OK             | |

### Testing

| Feature                  | opsys (C) | sysrust (Rust) | Notes |
|--------------------------|-----------|----------------|-------|
| Autotest mode            | OK        | OK             | |
| Serial test runner       | OK        | OK             | |
| make test                | OK        | OK             | |
| Test programs (23+)      | OK        | OK             | |

### Rust-Specific Quality

| Feature                  | opsys (C) | sysrust (Rust) | Notes |
|--------------------------|-----------|----------------|-------|
| unsafe_op_in_unsafe_fn   | N/A       | TODO           | Allowed globally, should fix per-fn |
| static mut cleanup       | N/A       | TODO           | 118 instances |

---

## Milestone Roadmap

### M1: sysrust Bootable -- DONE (2026-03-29)

- [x] Fix linker.ld KEEP()
- [x] Fix spinlock memory barriers
- [x] Add heap/VGA/ARP spinlocks
- [x] VGA volatile writes
- [x] Initial commit + push

### M2: sysrust Zero C + Rust-Syntax Compiler -- DONE (2026-03-29)

- [x] mkinitrd.rs replaces mkinitrd.c
- [x] Rust-syntax lexer + parser (rc/)
- [x] 21 .rs test programs, 15/15 autotest pass
- [x] Zero C files in repository

### M3: sysrust x86_64 Port -- DONE (2026-03-30)

- [x] 64-bit long mode (PAE, EFER.LME, 4-level paging)
- [x] 64-bit ISR stubs, IDT, GDT, context switch
- [x] ELF64 loader + Linux syscall instruction support
- [x] sysrust-rt runtime crate (allocator, syscall wrappers)
- [x] Real Rust programs with alloc (Vec, String, format!, closures)

### M4: sysrust OS Infrastructure for rustc -- DONE (2026-04-02)

- [x] 4GB RAM support (PMM, 4GB identity map, QEMU -m 4096)
- [x] VFS directories (mkdir, resolve_path, unlink, rename)
- [x] 9 new syscalls (lseek, fstat, getcwd, mkdir, getdents64, getrandom, rename, unlink, unlinkat)
- [x] Free-list allocator for sysrust-rt (dealloc support)

### M5: sysrust rustc Self-Hosting -- IN PROGRESS

Official rustc compiles itself inside sysrust. See docs/RUSTC_SELFHOST_PLAN.md.

- [x] Fork rust-lang/rust with sysrust target spec + PAL
- [ ] Fix stage0/source version mismatch (re-clone at release tag)
- [ ] Build std for x86_64-unknown-sysrust
- [ ] Cross-compile rustc (cranelift backend) for sysrust
- [ ] Run rustc inside sysrust
- [ ] Build sysrust-link (Rust-based ELF linker)
- [ ] Self-hosting bootstrap: rustc compiles itself

### M6: opsys Self-Hosting Compiler

The C compiler inside opsys can compile its own source.

- [ ] Remaining C language features needed for self-hosting
- [ ] Multi-file compilation or large-file support
- [ ] Self-hosting test

---

## Divergence Log

| Date | Feature | opsys | sysrust | Reason |
|------|---------|-------|---------|--------|
| 2026-03-29 | Guest language | C | Rust subset | Each OS is pure in its own language |
| 2026-03-29 | Built-in compiler | cc (C compiler) | rc (Rust compiler) | Language purity |
| 2026-03-29 | Host tools | mkinitrd.c | mkinitrd.rs | No C code in sysrust |
| 2026-03-29 | Test programs | .c files | .rs files | Match guest language |
| 2026-03-29 | Serial port | :2323 | :2324 | Avoid conflict when running side-by-side |
| 2026-03-30 | Architecture | i686 (32-bit) | x86_64 (64-bit) | Required for cranelift backend |
| 2026-03-30 | ELF loader | None | ELF32+ELF64 | Runs cross-compiled Rust binaries |
| 2026-04-01 | Linux syscall compat | None | syscall instruction | Enables running Linux-target binaries |
| 2026-04-02 | RAM | 512MB max | 4GB | Required for rustc compilation |
| 2026-04-02 | VFS directories | Flat root only | Full directory tree | Required for rustc source tree |
