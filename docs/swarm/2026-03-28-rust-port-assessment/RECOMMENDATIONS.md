# Final Recommendations: Rust OS Port

Generated: 2026-03-28
Merged from: draft-critical-high.md (R-001 through R-008) and draft-medium-low.md (R-100 through R-117)

---

## Summary Table

| Priority | Count | Category |
|----------|-------|----------|
| P0 | 1 | Boot-blocking |
| P1 | 5 | Data corruption/crash |
| P2 | 1 | Future build breakage |
| P3 | 7 | Should fix (correctness/quality) |
| P4 | 6 | Nice to have |
| P5 | 5 | Informational/cleanup |
| **Total** | **25** | |

Port-introduced: 19 of 25. Pre-existing (inherited from C): 6 of 25.

---

## P0 -- Boot-Blocking

### R-001: Multiboot header silently garbage-collected -- kernel cannot boot

| Field | Value |
|---|---|
| **Priority** | P0 (boot-blocking) |
| **Source findings** | boot-and-build #1 |
| **Dependencies** | None |

#### Root cause

The target spec `x86-opsys.json` line 15 passes `--gc-sections` to the linker. The linker script `linker.ld` line 26 includes the multiboot section as `*(.multiboot)` without `KEEP()`. Because no code references the multiboot header symbol, the linker treats the entire `.multiboot` section as dead data and discards it. The resulting ELF has no Multiboot magic in its first 8192 bytes, so QEMU cannot identify it as a Multiboot kernel. The kernel is completely unbootable.

The C original does not pass `--gc-sections`, so it never encounters this problem.

#### Fix

**File:** `linker.ld`, line 26

Change:
```
        *(.multiboot)
```
To:
```
        KEEP(*(.multiboot))
```

No other changes required. The `--gc-sections` flag in `x86-opsys.json` is otherwise beneficial and should be retained.

#### Verification

1. Rebuild: `cargo build --release --target x86-opsys.json`
2. Confirm the multiboot header is present:
   ```
   readelf -S target/x86-opsys/release/sysrust | grep multiboot
   ```
   The `.multiboot` section must have a non-zero size (at least 12 bytes).
3. Confirm the magic bytes are in the first 8KB:
   ```
   xxd target/x86-opsys/release/sysrust | grep "1bad b002"
   ```
4. Boot with QEMU:
   ```
   qemu-system-i386 -kernel target/x86-opsys/release/sysrust -nographic
   ```
   The kernel must proceed past the "Not booted via Multiboot!" check and begin subsystem init.

---

## P1 -- Data Corruption / Crash Risk

### R-002: `options(nomem)` on spinlock inline assembly defeats memory barrier

| Field | Value |
|---|---|
| **Priority** | P1 (data corruption risk) |
| **Source findings** | concurrency-safety #1, correctness H-1 |
| **Dependencies** | None (this is the foundation; R-003, R-004, R-006 depend on this) |

#### Root cause

The `Spinlock::lock()` and `Spinlock::unlock()` methods in `src/sync.rs` use `options(nomem)` on their inline assembly. This annotation tells LLVM that the asm block does not read or write memory, which is the opposite of what a synchronization primitive requires. LLVM is free to hoist stores past `cli` or sink loads past `popfd`, breaking every critical section that uses a spinlock.

The C original relies on GCC's implicit memory clobber for `asm volatile`, which acts as a full compiler barrier. The Rust `asm!` macro is volatile by default, but `options(nomem)` explicitly removes the memory-barrier property.

#### Fix

**File:** `src/sync.rs`

Line 50 -- in `lock()`, change:
```
            options(nomem),
```
To:
```
            options(nostack),
```

Line 68 -- in `unlock()`, change:
```
            options(nomem),
```
To:
```
            options(nostack),
```

Also fix the same `options(nomem)` in the Mutex methods for consistency:

Line 105 -- in `Mutex::lock()` first asm block, change:
```
                options(nomem),
```
To:
```
                options(nostack),
```

Line 115 -- in `Mutex::lock()` second asm block, change:
```
                options(nomem),
```
To:
```
                options(nostack),
```

Line 124 -- in `Mutex::lock()` third asm block, change:
```
                options(nomem),
```
To:
```
                options(nostack),
```

Remove `nomem` from all five asm blocks in `src/sync.rs`. Retain `nostack` (the pushfd/popfd pair does not net-change the stack pointer). Drop `preserves_flags` from `lock()` since `cli` modifies the interrupt flag.

#### Verification

1. Rebuild and disassemble the spinlock methods:
   ```
   objdump -d target/x86-opsys/release/sysrust | grep -A 20 "spinlock"
   ```
   Confirm no memory operations have been moved across the `cli` or `popfd` instructions.
2. Run with `opt-level = 2` (release profile) to stress the optimizer.
3. Boot and exercise concurrent paths (heap allocation during timer interrupts, VGA writes during keyboard interrupts) to confirm no hangs or corruption.

---

### R-003: Heap allocator (kmalloc/kfree) missing spinlock protection

| Field | Value |
|---|---|
| **Priority** | P1 (data corruption risk) |
| **Source findings** | concurrency-safety #2, correctness M-1 |
| **Dependencies** | R-002 (spinlock must be correct before it can be used here) |

#### Root cause

The C heap wraps `kmalloc` and `kfree` in `spin_lock(&heap_lock)` / `spin_unlock(&heap_lock)` to disable interrupts during allocation. The Rust port has no locking at all. The timer fires at 100 Hz and triggers preemptive scheduling every 10 ticks. If a timer interrupt fires mid-kmalloc and the scheduler context-switches to another thread that also calls kmalloc or kfree, the heap's linked-list metadata will be corrupted (torn block splits, lost free bytes, infinite loops in the free-list walk).

#### Fix

**File:** `src/heap.rs`

1. Add an import at the top of the file (after line 18):
```rust
use crate::sync::Spinlock;
```

2. Add a static lock after line 49 (`static mut HEAP_PAGES`):
```rust
static mut HEAP_LOCK: Spinlock = Spinlock::new();
```

3. In `kmalloc()` (starts at line 158), wrap the body with lock/unlock. After `if size == 0 { return ...; }` (line 160), add:
```rust
    HEAP_LOCK.lock();
```
Before every `return` statement in `kmalloc()` (lines 171, 184, 197), and before the final return, insert:
```rust
    HEAP_LOCK.unlock();
```

4. In `kfree()` (starts at line 200), wrap the body with lock/unlock. After the null check (line 202), add:
```rust
    HEAP_LOCK.lock();
```
Before every `return` in `kfree()` (line 208 for double-free guard), insert `HEAP_LOCK.unlock();`, and add `HEAP_LOCK.unlock();` after the `coalesce()` call (line 212).

#### Verification

1. The kernel must boot and complete all init steps without hanging (proves the lock does not deadlock during init-time allocations).
2. Stress test: spawn multiple threads that each perform many small allocations and frees. Verify `heap::get_free()` returns a consistent value after all threads complete.
3. Run autotest mode and confirm all output matches the expected strings.

---

### R-004: VGA lock is a plain bool -- no interrupt disable, no atomicity

| Field | Value |
|---|---|
| **Priority** | P1 (deadlock / crash risk) |
| **Source findings** | concurrency-safety #4 |
| **Dependencies** | R-002 (spinlock must be correct before it can be used here) |

#### Root cause

The VGA module implements locking with `static mut VGA_LOCKED: bool` and a busy-wait loop. This has three defects: (1) no interrupt disabling -- if an IRQ handler (panic path, keyboard echo) calls VGA code while the lock is held by mainline code, it will spin forever (hard deadlock); (2) not atomic -- the compiler may cache the bool in a register, making the spin loop infinite; (3) no saved interrupt state -- the C version saves and restores EFLAGS.

The `src/sync.rs` module already provides a `Spinlock` type that handles all three concerns.

#### Fix

**File:** `src/vga.rs`

1. Add an import at the top (after line 15):
```rust
use crate::sync::Spinlock;
```

2. Replace line 50:
```rust
static mut VGA_LOCKED: bool = false;
```
With:
```rust
static mut VGA_LOCK: Spinlock = Spinlock::new();
```

3. Replace the `lock()` function (lines 54-59):
```rust
#[inline]
unsafe fn lock() {
    VGA_LOCK.lock();
}
```

4. Replace the `unlock()` function (lines 63-65):
```rust
#[inline]
unsafe fn unlock() {
    VGA_LOCK.unlock();
}
```

#### Verification

1. Boot and confirm VGA output displays correctly during init.
2. Type rapidly on the keyboard while a thread is printing -- exercises the IRQ-vs-mainline contention path. No deadlock or garbled output should occur.
3. Trigger a panic path that prints to VGA while output is in progress -- confirm the kernel prints the panic message without hanging.

---

### R-005: VGA buffer writes are not volatile -- LLVM may eliminate or reorder them

| Field | Value |
|---|---|
| **Priority** | P1 (display corruption risk) |
| **Source findings** | concurrency-safety #5 |
| **Dependencies** | None (independent of lock fixes) |

#### Root cause

All writes to the VGA text buffer at physical address 0xB8000 use plain raw pointer dereference (`*ptr = val`) instead of `core::ptr::write_volatile()`. LLVM is permitted to coalesce, eliminate, or reorder these writes. For a memory-mapped display buffer, every write has a visible side effect. Other modules in the same codebase (timer.rs, keyboard.rs, icmp.rs, dns.rs) correctly use volatile access.

#### Fix

**File:** `src/vga.rs`

Line 96 (scroll copy):
```rust
        core::ptr::write_volatile(buf.add(i), core::ptr::read_volatile(buf.add(i + row_bytes)));
```

Line 101 (scroll blank fill):
```rust
        core::ptr::write_volatile(buf.add(last_row_start + i), vga_entry(b' ', CURRENT_COLOR));
```

Line 116 (backspace blank):
```rust
        core::ptr::write_volatile(VGA_BUFFER.add(offset), vga_entry(b' ', CURRENT_COLOR));
```

Line 120 (character write):
```rust
        core::ptr::write_volatile(VGA_BUFFER.add(offset), vga_entry(c, CURRENT_COLOR));
```

Line 169 (set_80x50 clear):
```rust
        core::ptr::write_volatile(VGA_BUFFER.add(i), vga_entry(b' ', CURRENT_COLOR));
```

Line 183 (clear screen):
```rust
        core::ptr::write_volatile(VGA_BUFFER.add(i), vga_entry(b' ', CURRENT_COLOR));
```

#### Verification

1. Rebuild with `opt-level = 2` and boot in QEMU.
2. Confirm the screen clears on init (proves the clear loop was not eliminated).
3. Scroll the screen by printing more than 50 lines -- confirm characters scroll correctly.
4. Press backspace -- confirm the character is erased.

---

### R-006: ARP table accessed from IRQ and mainline without spinlock

| Field | Value |
|---|---|
| **Priority** | P1 (data corruption risk) |
| **Source findings** | concurrency-safety #3 |
| **Dependencies** | R-002 (spinlock must be correct before it can be used here) |

#### Root cause

The `ARP_TABLE` global is accessed from two contexts without synchronization: `arp::rx()` calls `arp_table_update()` from the network IRQ handler, while `arp::resolve()` calls `arp_table_lookup()` from thread context. If a network IRQ fires during `arp_table_lookup`, the concurrent `arp_table_update` may produce torn reads of MAC addresses or inconsistent valid flags.

The C version wraps both callsites with `spin_lock(&arp_lock)` / `spin_unlock(&arp_lock)`.

#### Fix

**File:** `src/arp.rs`

1. Add an import at the top (after line 13):
```rust
use crate::sync::Spinlock;
```

2. Add a static lock after the `ARP_TABLE` declaration (after line 43):
```rust
static mut ARP_LOCK: Spinlock = Spinlock::new();
```

3. In `arp_table_update()` (line 57), add `ARP_LOCK.lock();` at the start and `ARP_LOCK.unlock();` before every `return`.

4. In `arp_table_lookup()` (line 81), add `ARP_LOCK.lock();` at the start and `ARP_LOCK.unlock();` before every `return`.

#### Verification

1. Boot with QEMU user-mode networking.
2. Issue a DNS lookup or HTTP request that triggers ARP resolution while network traffic is arriving. Confirm the ARP table resolves correctly and the kernel does not hang.
3. Repeatedly call `arp::resolve()` from multiple threads while pinging the kernel. Verify no corruption.

---

## P2 -- Future Build Breakage

### R-008: `unsafe_op_in_unsafe_fn` -- 3308 warnings becoming future hard errors

| Field | Value |
|---|---|
| **Priority** | P2 (build breakage on future toolchain) |
| **Source findings** | boot-and-build #2 |
| **Dependencies** | None |

#### Root cause

The port uses `edition = "2024"` which activates the `unsafe_op_in_unsafe_fn` lint as a warning. All 3308 instances of unsafe operations inside `unsafe fn` bodies lack inner `unsafe {}` blocks. Breakdown by file: `src/cc/parse.rs` (1093), `src/shell.rs` (487), `src/cc/lex.rs` (343), `src/fat16.rs` (240), `src/cc/mod.rs` (183), `src/editor.rs` (177), and others.

These warnings are on track to become hard errors in a future nightly, at which point the kernel will fail to compile.

#### Fix

**File:** `src/main.rs`, line 9

As an immediate stopgap, add:
```rust
#![allow(unsafe_op_in_unsafe_fn)]
```

For the long-term fix: add inner `unsafe {}` blocks around each unsafe operation in `unsafe fn` bodies, starting with the most affected files. This is a mechanical transformation that can be done incrementally.

#### Verification

1. Rebuild and confirm zero `unsafe_op_in_unsafe_fn` warnings in the build output.
2. Pin the current nightly toolchain version in `rust-toolchain.toml` to prevent surprise breakage.

---

## P3 -- Medium Priority (Should Fix)

### R-007: Mutex extern declarations reference non-existent symbols

| Field | Value |
|---|---|
| **Priority** | P1 (link failure if Mutex is used) |
| **Source findings** | concurrency-safety #10, correctness H-2 |
| **Dependencies** | None |

#### Root cause

The Rust `Mutex` in `src/sync.rs` (lines 17-20) declares `extern "C" { fn thread_get_id() -> i32; fn thread_yield(); }`, but the Rust thread module exports its functions as `thread::get_current()` and `thread::yield_thread()` without `#[no_mangle]`. No symbols named `thread_get_id` or `thread_yield` exist in the binary. If the Mutex is ever instantiated and its `lock()` method is called, the build will fail with undefined symbol errors.

#### Fix

**File:** `src/sync.rs`

Replace the extern block (lines 17-20) with direct Rust module calls:
```rust
use crate::thread;
```

Then in `Mutex::lock()`, replace `thread_get_id()` with `thread::get_current()`, and replace `thread_yield()` with `thread::yield_thread()`.

#### Verification

1. Confirm the build succeeds with `cargo build --release --target x86-opsys.json`.
2. If any code uses Mutex, confirm it compiles and links.

---

### R-100: TCP `rx_head` missing `write_volatile` in IRQ handler

| Field | Value |
|---|---|
| **Priority** | P3 |
| **Source findings** | concurrency-safety Finding 7, rust-quality Finding 7a |
| **Dependencies** | None |

#### Root cause

The C version declares `rx_head` as `volatile uint16`, which covers all access sites automatically. The Rust port reads `CONNS[i].rx_head` with `read_volatile` in mainline `recv()` but writes it with plain assignment in the IRQ handler `rx()`. This asymmetry means the compiler may defer, reorder, or elide the store in the IRQ path, causing mainline code to see a stale `rx_head` and lose data or desynchronize the ring buffer.

#### Fix

- File: `src/tcp.rs`, line ~451
- Change: `CONNS[i].rx_head = next;`
- To: `core::ptr::write_volatile(&raw mut CONNS[i].rx_head, next);`

**Port-introduced:** Yes.

---

### R-101: Split `asm!` blocks for interrupt disable in `send_ethernet`

| Field | Value |
|---|---|
| **Priority** | P3 |
| **Source findings** | concurrency-safety Finding 8, rust-quality Finding 9 |
| **Dependencies** | None |

#### Root cause

The C version uses a single `asm volatile("pushfl; popl %0; cli")`. The Rust port splits this into two separate `asm!` blocks. The compiler may insert spills, reloads, or other generated code between the two blocks. An interrupt arriving in that window would execute with interrupts enabled.

#### Fix

- File: `src/net.rs`, lines ~109-110
- Combine into a single asm block:
  ```rust
  asm!("pushfd", "pop {0:e}", "cli", out(reg) flags);
  ```

**Port-introduced:** Yes.

---

### R-102: Function pointer casts directly to integer (31 instances)

| Field | Value |
|---|---|
| **Priority** | P3 |
| **Source findings** | rust-quality Finding 2 |
| **Dependencies** | None |

#### Root cause

The compiler symbol table registers 31 built-in functions by casting function items directly to `u32` (`builtin_puts as u32`). Rust warns that function items should not be cast directly to integers because it bypasses the intermediate pointer type.

#### Fix

- File: `src/cc/sym.rs`, lines ~160-204
- Mechanical replacement:
  ```rust
  // Before
  add_builtin(b"puts\0", builtin_puts as u32);
  // After
  add_builtin(b"puts\0", builtin_puts as *const () as u32);
  ```

**Port-introduced:** Yes.

---

### R-103: Pervasive `static mut` with `#![allow(static_mut_refs)]`

| Field | Value |
|---|---|
| **Priority** | P3 |
| **Source findings** | rust-quality Finding 4 |
| **Dependencies** | None |

#### Root cause

The project has 118 `static mut` declarations across all 31 source files. A project-wide `#![allow(static_mut_refs)]` suppresses hundreds of warnings. Taking a reference to a `static mut` is unsound under Rust's aliasing model, and `static mut` is being deprecated.

#### Fix (incremental)

1. Start with interrupt-shared state: replace with `core::sync::atomic` types where possible.
2. For larger shared structures: access through raw pointers via `core::ptr::addr_of_mut!()`.
3. For module-local state: consider `UnsafeCell` wrappers.
4. Remove the blanket `#![allow(static_mut_refs)]` only after migration is complete.

Priority files: `src/sync.rs`, `src/tcp.rs`, `src/arp.rs`, `src/timer.rs`, `src/heap.rs`.

**Port-introduced:** Yes.

---

### R-104: Inconsistent volatile access for RTL8139 IRQ-shared state

| Field | Value |
|---|---|
| **Priority** | P3 |
| **Source findings** | concurrency-safety Finding 12, rust-quality Finding 7b |
| **Dependencies** | None |

#### Root cause

The RTL8139 driver's `RX_OFFSET`, `CURRENT_TX`, and `IO_BASE` statics are accessed from both IRQ context and mainline context without volatile semantics. While current call patterns make data races unlikely, any change in calling patterns would introduce silent races.

#### Fix

- File: `src/rtl8139.rs`
- Use `read_volatile`/`write_volatile` for all accesses to `RX_OFFSET` in `rtl8139_rx()` and any mainline readers.

**Port-introduced:** No (pre-existing; C version also lacks `volatile` here).

---

### R-105: Ramfs capacity doubling can overflow u32

| Field | Value |
|---|---|
| **Priority** | P3 |
| **Source findings** | correctness Finding M-2 |
| **Dependencies** | None |

#### Root cause

The buffer growth loop `while new_cap < needed { new_cap *= 2; }` can cause `new_cap` to wrap around zero if `needed` is close to `u32::MAX`.

#### Fix

- File: `src/ramfs.rs` (buffer growth loop)
- Add an overflow check:
  ```rust
  while new_cap < needed {
      new_cap = match new_cap.checked_mul(2) {
          Some(v) => v,
          None => return -1,
      };
  }
  ```

**Port-introduced:** No (pre-existing; identical logic in C original).

---

### R-106: Rust 2024 `unsafe_op_in_unsafe_fn` warnings (3308 instances)

| Field | Value |
|---|---|
| **Priority** | P3 |
| **Source findings** | rust-quality Finding 1 |
| **Dependencies** | None |

**Note:** This overlaps with R-008 (the P2 stopgap). R-008 covers the immediate `#![allow(...)]` fix. This entry covers the long-term mechanical transformation of all 3308 sites.

#### Root cause

The project uses `edition = "2024"` which requires explicit `unsafe {}` blocks inside `unsafe fn` bodies. Most-affected files: `src/cc/parse.rs` (1093), `src/shell.rs` (487), `src/cc/lex.rs` (343), `src/fat16.rs` (240), `src/cc/mod.rs` (183), `src/editor.rs` (177).

#### Fix

- Scope: All 31 source files.
- Mechanical transformation: wrap unsafe operations in explicit `unsafe {}` blocks.
- Tooling: `cargo clippy --fix` with the `unsafe_op_in_unsafe_fn` lint may automate much of this.

**Port-introduced:** Yes.

---

## P4 -- Low Priority (Nice to Have)

### R-107: Spinlock `locked` field not volatile/atomic

| Field | Value |
|---|---|
| **Priority** | P4 |
| **Source findings** | concurrency-safety Finding 6, correctness Finding L-7 |
| **Dependencies** | None |

#### Root cause

The C version declares `volatile uint32 locked` in `spinlock_t`. The Rust version uses plain `u32`. Since the spinlock uses `cli`/`sti` and the `locked` field is only accessed within lock/unlock methods (which contain inline asm), the compiler is unlikely to elide the write.

#### Fix

- File: `src/sync.rs`
- Change `locked: u32` to `locked: core::sync::atomic::AtomicU32`
- Use `Ordering::Relaxed` for loads/stores.

**Port-introduced:** Yes.

---

### R-108: Double EOI for timer and keyboard IRQs

| Field | Value |
|---|---|
| **Priority** | P4 |
| **Source findings** | concurrency-safety Finding 9, correctness Findings L-1 and L-2 |
| **Dependencies** | None |

#### Root cause

The Rust IRQ handlers each call `pic::send_eoi()` explicitly, but the central `isr_handler` in `idt.rs` also sends EOI for all vectors 32-47. The C version only sends EOI in the central dispatch. Additionally, the timer handler sends EOI *before* `thread_yield()`, whereas the C version sends it *after*.

#### Fix

- File: `src/timer.rs`, line ~30 -- remove `pic::send_eoi(0)` call
- File: `src/keyboard.rs`, lines ~66, 73, 79, 85, 98, 110 -- remove all `pic::send_eoi(1)` calls
- Rely on the central EOI dispatch in `src/idt.rs`.

**Port-introduced:** Yes.

---

### R-109: Packed struct direct field assignment (technical UB)

| Field | Value |
|---|---|
| **Priority** | P4 |
| **Source findings** | concurrency-safety Finding 11, correctness Finding M-3, rust-quality Finding 3a |
| **Dependencies** | None |

#### Root cause

Assigning to a field of a `#[repr(C, packed)]` struct may create an intermediate reference to a misaligned address. Two files have direct assignment: `src/arp.rs` and `src/icmp.rs`. All affected `u16` fields happen to fall at even offsets, so no actual misalignment occurs today.

#### Fix

- Files: `src/arp.rs`, `src/icmp.rs`
- Replace: `pkt.field = value;`
- With: `core::ptr::write_unaligned(core::ptr::addr_of_mut!(pkt.field), value);`
- Approximately 14 individual assignments.

**Port-introduced:** Yes.

---

### R-110: FAT16 VFS write integer overflow

| Field | Value |
|---|---|
| **Priority** | P4 |
| **Source findings** | correctness Finding L-4 |
| **Dependencies** | None |

#### Root cause

In the FAT16 VFS write callback, `new_size = offset + size` can overflow `u32`.

#### Fix

- File: `src/fat16_vfs.rs`
- Add overflow check:
  ```rust
  let new_size = match offset.checked_add(size) {
      Some(v) => v,
      None => return -1,
  };
  ```

**Port-introduced:** No (pre-existing).

---

### R-111: Fat16DirEntry packed struct access pattern

| Field | Value |
|---|---|
| **Priority** | P4 |
| **Source findings** | correctness Finding M-3 |
| **Dependencies** | None |

#### Root cause

`Fat16DirEntry` uses `#[repr(C, packed)]` and code accesses fields directly. On Rust 1.82+ the compiler generates direct stores for simple value copies. No actual UB today, but fragile.

#### Fix

- File: `src/fat16.rs`
- Add a safety comment on the struct. Consider proactively switching to `read_unaligned` for multi-byte fields.

**Port-introduced:** Yes.

---

### R-112: Mutex extern declarations reference non-existent symbols (duplicate of R-007)

| Field | Value |
|---|---|
| **Priority** | P4 |
| **Source findings** | concurrency-safety Finding 10, correctness Finding H-2 |
| **Dependencies** | None |

**Note:** This was identified independently in the medium-low draft. See R-007 for the primary recommendation and fix. This entry is retained for traceability.

**Port-introduced:** Yes.

---

## P5 -- Informational / Cleanup

### R-113: Dead code warnings (35 items)

| Field | Value |
|---|---|
| **Priority** | P5 |
| **Source findings** | rust-quality Finding 10 |
| **Dependencies** | None |

35 functions, constants, structs, and enum variants are defined but never used. For items that should be used (e.g., `Spinlock`): wire them up. For items intentionally reserved: annotate individually with `#[allow(dead_code)]`. For genuinely unused items: remove them.

**Port-introduced:** Partially.

---

### R-114: /dev/zero read and /dev/null write return negative for size > 2GB

| Field | Value |
|---|---|
| **Priority** | P5 |
| **Source findings** | correctness Finding L-5 |
| **Dependencies** | None |

- File: `src/devfs.rs`
- Cap the return value: `return core::cmp::min(size, 0x7FFFFFFF) as i32;`

**Port-introduced:** No (pre-existing).

---

### R-115: PIT_DIVISOR off by 1

| Field | Value |
|---|---|
| **Priority** | P5 |
| **Source findings** | correctness Finding L-3 |
| **Dependencies** | None |

C computes `1193182 / 100 = 11931`. Rust hardcodes `11932`. The difference is <0.01% in timer frequency.

- File: `src/timer.rs`, line 17
- Change `11932` to `11931` to match C exactly, or document the intentional rounding.

**Port-introduced:** Yes (intentional rounding choice).

---

### R-116: Loss of short I/O port encoding optimization

| Field | Value |
|---|---|
| **Priority** | P5 |
| **Source findings** | rust-quality Finding 11 |
| **Dependencies** | None |

Rust `asm!` lacks the GCC `N` constraint for short port encoding. No fix available. No correctness impact.

**Port-introduced:** Yes (Rust language limitation).

---

### R-117: fat_alloc u16 loop variable

| Field | Value |
|---|---|
| **Priority** | P5 |
| **Source findings** | correctness Finding L-6 |
| **Dependencies** | None |

The loop variable in `fat_alloc` is `u16`. Standard FAT16 volumes have at most 65527 data entries, so this is within range.

- File: `src/fat16.rs`
- Widen loop variable to `u32` for safety margin, or document the constraint.

**Port-introduced:** No (pre-existing).

---

## Fix Order

The recommended fix sequence, accounting for dependencies:

```
Phase 1 -- Boot blocker (must fix first):
  R-001  linker.ld KEEP()

Phase 2 -- Concurrency foundation (unblocks Phase 3):
  R-002  sync.rs remove options(nomem)

Phase 3 -- Add missing locks (can be done in parallel):
  R-003  heap.rs add HEAP_LOCK              ---> depends on R-002
  R-004  vga.rs replace bool with Spinlock  ---> depends on R-002
  R-006  arp.rs add ARP_LOCK               ---> depends on R-002

Phase 4 -- Volatile and linkage (independent of each other):
  R-005  vga.rs volatile writes
  R-007  sync.rs fix Mutex imports
  R-100  tcp.rs rx_head write_volatile
  R-101  net.rs combine split asm blocks

Phase 5 -- Build hygiene (stopgap):
  R-008  main.rs allow unsafe_op_in_unsafe_fn

Phase 6 -- Medium priority (incremental):
  R-102  cc/sym.rs two-step function pointer casts
  R-105  ramfs.rs overflow check
  R-104  rtl8139.rs volatile access
  R-103  static mut migration (long-term, incremental)
  R-106  unsafe_op_in_unsafe_fn full fix (long-term)

Phase 7 -- Low priority (opportunistic):
  R-108  timer.rs/keyboard.rs remove duplicate EOI
  R-109  arp.rs/icmp.rs write_unaligned for packed structs
  R-107  sync.rs AtomicU32 for locked field
  R-110  fat16_vfs.rs overflow check
  R-111  fat16.rs packed struct comment
  R-112  (covered by R-007)

Phase 8 -- Cleanup (as convenient):
  R-113  Dead code cleanup
  R-114  devfs.rs size cap
  R-115  timer.rs PIT_DIVISOR
  R-116  (no fix available)
  R-117  fat16.rs widen loop variable
```

### Dependency Graph

```
R-001 (boot blocker)
  |
  v
R-002 (spinlock nomem fix)
  |
  +---> R-003 (heap lock)
  +---> R-004 (VGA lock)
  +---> R-006 (ARP lock)

R-005 (VGA volatile) ----------> independent
R-007 (Mutex imports) ---------> independent
R-008 (allow lint) ------------> independent
R-100 (TCP volatile) ----------> independent
R-101 (split asm) -------------> independent
R-102..R-117 ------------------> independent (no blocking dependencies)
```
