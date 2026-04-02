# Plan: Self-Hosting Rust Compiler Inside sysrust

## The Goal

A Rust compiler runs inside sysrust, compiles itself, that copy compiles itself again. Minimal Linux/libc.

## Why Not Official rustc

Research findings:
- **rustc cannot be no_std** — 500K lines using std everywhere
- **rustc needs 1.5-4 GB RAM** to compile itself — our OS has 512MB
- **rustc MUST spawn a linker** (fork+exec) — we have no process spawning
- **A std shim (2-5K lines) doesn't solve** the linker or memory problems
- **Redox's approach took 6+ years** with a team and built a full Unix

Running the official rustc binary inside sysrust requires building a Unix. That's not the goal.

## The Actual Path: Cranelift-Based Rust Compiler

**Key insight: Cranelift supports `no_std` + `alloc`.** It runs on our OS today.

Build a **new Rust compiler** that:
1. Supports enough Rust to compile its own source (generics, traits, impl, enums, match, closures)
2. Uses **Cranelift** as the code generation backend (~200K lines, all no_std)
3. Produces ELF files **in-process** via `cranelift-object` — **no linker needed**
4. Runs on sysrust-rt (our allocator + syscall wrappers)
5. Uses ~50-200 MB RAM (fits in our 512MB)
6. Total size: ~8,000-15,000 lines

This is NOT the official rustc. But it compiles Rust, produces native x86_64 code, and bootstraps itself. It's a real compiler with a real backend.

## Architecture

```
.rs source → Lexer → Parser → AST → Type Check → MIR → Cranelift IR → x86_64 ELF
                                                         ↑
                                                   cranelift-codegen (no_std, from crates.io)
                                                   cranelift-object (produces ELF in-process)
```

## Components (~12K lines estimated)

| Component | Lines | What it does |
|-----------|-------|-------------|
| Lexer | ~800 | Tokenize Rust source (extend existing rc/lex.rs) |
| Parser | ~3,500 | Generics, traits, impl, enum, match, closures, modules |
| Name resolution | ~1,000 | Resolve paths, imports, trait impls |
| Type checker | ~2,500 | Type inference, monomorphization, trait dispatch |
| MIR lowering | ~1,500 | Translate typed AST → Cranelift CLIF IR |
| ELF output | ~500 | cranelift-object writes ELF, symbol resolution |
| Driver | ~500 | CLI, file I/O, orchestration |
| Runtime | ~200 | Panic handler, allocator (sysrust-rt already exists) |

## Prerequisites (do first)

1. **Replace bump allocator with free-list** — cranelift and the compiler need `dealloc` to work. The current bump allocator leaks all memory. ~200 lines to add free-list to sysrust-rt.

2. **Verify cranelift compiles as no_std** — cross-compile `cranelift-codegen` for `x86_64-unknown-none` with our sysrust-rt. If it works, the path is viable. If not, we need to identify what's blocking.

3. **Add lseek + file write to sysrust-rt** — the compiler needs to create output files. Add write-file syscall wrapper.

## Bootstrap Sequence

```
Stage 0: Host rustc cross-compiles our compiler → ELF binary (runs on sysrust)
Stage 1: Boot sysrust, load Stage 0 binary, it compiles its own source → Stage 1 binary
Stage 2: Run Stage 1 binary, it compiles its own source → Stage 2 binary
Verify: Stage 1 and Stage 2 produce identical output (bit-for-bit)
```

## Phased Implementation

### Phase 1: Prove Cranelift Works (~1 day)
- Cross-compile a hello-world using cranelift-codegen as no_std
- Run it inside sysrust
- If this fails, stop and reassess

### Phase 2: Fix the Allocator (~1 day)
- Replace bump allocator in sysrust-rt with free-list (supporting dealloc)
- Test with cranelift (it allocates and frees heavily)

### Phase 3: Build the Compiler Frontend (~2 weeks)
- Lexer (extend existing rc/lex.rs for full Rust tokens)
- Parser (generics, traits, impl, enum with data, match, closures, use/mod)
- Name resolution + type checking
- This is the bulk of the work

### Phase 4: Cranelift Backend (~1 week)
- Lower typed AST to Cranelift IR
- Use cranelift-object to write ELF
- Handle function calls, structs, enums, trait dispatch, generics

### Phase 5: Self-Hosting (~1 week)
- The compiler must support every Rust feature it uses in its own source
- Iterative: compile, find missing feature, implement, repeat
- When Stage 1 builds Stage 2 identically: done

## Why This Works

- **Cranelift is no_std** — confirmed, just needs `alloc`
- **No linker needed** — `cranelift-object` produces ELF in-process
- **No std needed** — the compiler itself is no_std
- **Fits in 512MB** — cranelift is 2-5MB binary, uses 50-200MB for compilation
- **No fork/exec** — everything runs in one process
- **No libc** — just our syscall wrappers

## Risks

1. **Cranelift might not fully compile as no_std** — some feature gate may pull in std. Mitigated by testing in Phase 1.
2. **The parser + type checker is a LOT of work** — ~7K lines of careful compiler engineering. This is months, not days.
3. **Self-hosting requires implementing every feature the compiler uses** — circular dependency that's hard to estimate upfront.

## Comparison with Alternatives

| Approach | Effort | Result |
|----------|--------|--------|
| Run official rustc (Linux compat) | 6+ months (build Unix) | Official rustc but massive OS work |
| Fork rustc + custom std shim | 3-6 months | Still needs fork/exec for linker |
| **Cranelift-based new compiler** | **3-6 months** | **Self-hosting, no_std, no linker, fits in 512MB** |
| mrustc port (C++ to Rust) | 6+ months | Mature but massive porting effort |

## Verification

Phase 1: `cargo build` of cranelift-codegen for x86_64-unknown-none succeeds
Phase 1: A program using cranelift runs inside sysrust and generates x86_64 code
Phase 5: Stage 1 compiler binary produces identical output to Stage 2
