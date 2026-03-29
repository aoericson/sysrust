# opsys / sysrust Feature Matrix

Last updated: 2026-03-29

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

### C Compiler (built-in)

| Feature                  | opsys (C) | sysrust (Rust) | Notes |
|--------------------------|-----------|----------------|-------|
| Lexer (67 tokens)        | OK        | OK             | |
| Parser (recursive desc)  | OK        | OK             | |
| x86 emitter              | OK        | OK             | |
| Symbol table (31 builtins)| OK       | OK             | |
| Types: int, char, void, ptr | OK     | OK             | |
| Control: if/else/while/for | OK      | OK             | |
| Operators (full set)     | OK        | OK             | |
| Strings + #define        | OK        | OK             | |
| Structs                  | OK        | OK             | |
| Ternary + do-while       | OK        | OK             | |
| Enum support             | OK        | TODO           | Post-v0.3, not yet ported |
| Switch/case              | OK        | TODO           | Post-v0.3, not yet ported |
| #include (dedup)         | OK        | OK             | sysrust ported at v0.3 level |
| typedef                  | OK        | TODO           | Post-v0.3, not yet ported |
| Function prototypes      | OK        | TODO           | Post-v0.3, not yet ported |
| static/const/extern kw   | OK        | TODO           | Post-v0.3, not yet ported |
| Array fields in structs  | OK        | TODO           | Post-v0.3, not yet ported |
| unsigned/signed/short/long| OK       | TODO           | Post-v0.3, not yet ported |

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

### M2: sysrust Feature Parity with opsys HEAD

Port post-v0.3 compiler features to sysrust.

- [ ] Enum support
- [ ] Switch/case
- [ ] typedef, unsigned/signed/short/long keywords
- [ ] Function prototypes
- [ ] static/const/extern keywords
- [ ] Array fields in structs

### M3: opsys Self-Hosting Compiler

The C compiler inside opsys can compile its own source.

- [ ] Remaining C language features needed for self-hosting
- [ ] Multi-file compilation or large-file support
- [ ] Self-hosting test

### M4: Shared Next-Generation Features

New features developed in both projects simultaneously.

- [ ] TCP server
- [ ] Ring 3 / userspace isolation
- [ ] ELF loading
- [ ] Process isolation (separate address spaces)

---

## Divergence Log

| Date | Feature | opsys | sysrust | Reason |
|------|---------|-------|---------|--------|
| 2026-03-29 | Post-v0.3 compiler | OK | TODO | Rust port created from v0.3 snapshot |
| 2026-03-29 | Serial port | :2323 | :2324 | Avoid conflict when running side-by-side |
