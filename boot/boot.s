; boot.s — Kernel entry point for x86_64
;
; Multiboot drops us in 32-bit protected mode. We must:
; 1. Set up bootstrap page tables (identity-map first 1GB with 2MB huge pages)
; 2. Enable PAE, long mode, paging
; 3. Load 64-bit GDT
; 4. Far jump to 64-bit code
; 5. Set up stack and call kernel_main

; Multiboot header (stays 32-bit, in .multiboot section)
MBALIGN  equ 1 << 0
MEMINFO  equ 1 << 1
FLAGS    equ MBALIGN | MEMINFO
MAGIC    equ 0x1BADB002
CHECKSUM equ -(MAGIC + FLAGS)

section .multiboot
align 4
    dd MAGIC
    dd FLAGS
    dd CHECKSUM

; Bootstrap page tables in BSS (must be page-aligned)
section .bss
align 4096
boot_pml4:    resb 4096
boot_pdpt:    resb 4096
boot_pd:      resb 4096

align 16
stack_bottom: resb 16384
stack_top:

section .note.GNU-stack noalloc noexec nowrite progbits

; 32-bit code section
section .text
bits 32
global _start
extern kernel_main

_start:
    ; Save multiboot info (EAX=magic, EBX=info pointer)
    mov edi, eax          ; save magic in EDI (will be RDI in 64-bit)
    mov esi, ebx          ; save info pointer in ESI (will be RSI in 64-bit)

    ; Set up bootstrap page tables for identity-mapping first 1GB
    ; PML4[0] -> boot_pdpt
    mov eax, boot_pdpt
    or  eax, 0x03         ; present + writable
    mov [boot_pml4], eax

    ; PDPT[0] -> boot_pd
    mov eax, boot_pd
    or  eax, 0x03
    mov [boot_pdpt], eax

    ; PD[0..511] -> 2MB huge pages identity-mapping 0..1GB
    mov ecx, 0            ; counter
    mov eax, 0x83         ; present + writable + page_size(2MB)
.fill_pd:
    mov [boot_pd + ecx*8], eax
    mov dword [boot_pd + ecx*8 + 4], 0  ; high 32 bits = 0
    add eax, 0x200000     ; next 2MB page
    inc ecx
    cmp ecx, 512
    jne .fill_pd

    ; Load PML4 into CR3
    mov eax, boot_pml4
    mov cr3, eax

    ; Enable PAE (CR4 bit 5)
    mov eax, cr4
    or  eax, 1 << 5
    mov cr4, eax

    ; Enable long mode (EFER.LME, MSR 0xC0000080 bit 8)
    mov ecx, 0xC0000080
    rdmsr
    or  eax, 1 << 8
    wrmsr

    ; Enable paging (CR0 bit 31) — activates long mode
    mov eax, cr0
    or  eax, 1 << 31
    mov cr0, eax

    ; Load 64-bit GDT
    lgdt [gdt64_ptr]

    ; Far jump to 64-bit code segment
    jmp 0x08:long_mode_start

; 64-bit code
bits 64
long_mode_start:
    ; Set up data segment registers
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax

    ; Enable SSE (required by x86_64 ABI)
    mov rax, cr0
    and ax, 0xFFFB       ; clear CR0.EM (bit 2)
    or  ax, 0x0002       ; set CR0.MP (bit 1)
    mov cr0, rax
    mov rax, cr4
    or  ax, 3 << 9       ; set CR4.OSFXSR (bit 9) and CR4.OSXMMEXCPT (bit 10)
    mov cr4, rax

    ; Set up stack
    mov rsp, stack_top

    ; Zero-extend saved multiboot values (already in EDI/ESI, upper bits zeroed by CPU)
    ; RDI = multiboot magic, RSI = multiboot info pointer
    ; (System V AMD64 calling convention: args in RDI, RSI)

    call kernel_main

.hang:
    cli
    hlt
    jmp .hang

; 64-bit GDT
align 8
gdt64:
    dq 0                          ; null descriptor
    dq 0x00AF9A000000FFFF         ; 64-bit code: base=0, limit=0xFFFFF, L=1, D=0, P=1, DPL=0, S=1, type=0xA
    dq 0x00CF92000000FFFF         ; 64-bit data: base=0, limit=0xFFFFF, G=1, P=1, DPL=0, S=1, type=0x2
gdt64_end:

gdt64_ptr:
    dw gdt64_end - gdt64 - 1     ; limit
    dq gdt64                      ; base (64-bit)

; gdt_flush — Load a new GDT and reload segment registers.
; Argument in RDI (pointer to GDT descriptor)
global gdt_flush
gdt_flush:
    lgdt [rdi]
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax
    ; Far return to reload CS with 0x08
    ; Use retfq trick: push segment, push return address, retfq
    pop rax              ; get return address
    push qword 0x08      ; new CS
    push rax             ; return address
    retfq

; context_switch — Switch between kernel threads (x86_64)
; RDI = pointer to save current RSP (*old_rsp)
; RSI = new RSP to switch to
global context_switch
context_switch:
    ; Save callee-saved registers on current stack
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15

    ; Save current stack pointer
    mov [rdi], rsp

    ; Switch to new stack
    mov rsp, rsi

    ; Restore callee-saved registers from new stack
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx

    ret
