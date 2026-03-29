; boot.s — Kernel entry point and assembly helpers.
;
; This is the first code that executes when QEMU loads our kernel. It provides:
;   1. The Multiboot header (so QEMU recognizes this as a bootable kernel)
;   2. _start: sets up the stack and calls kernel_main
;   3. gdt_flush: reloads segment registers after a GDT change
;   4. context_switch: saves/restores thread state for the scheduler

; ---------------------------------------------------------------------------
; Multiboot header
; ---------------------------------------------------------------------------
MBALIGN  equ 1 << 0            ; flag: align loaded modules on page boundaries
MEMINFO  equ 1 << 1            ; flag: provide a memory map to the kernel
FLAGS    equ MBALIGN | MEMINFO
MAGIC    equ 0x1BADB002        ; the Multiboot magic number
CHECKSUM equ -(MAGIC + FLAGS)  ; header checksum (must sum to zero with above)

section .multiboot
align 4
    dd MAGIC
    dd FLAGS
    dd CHECKSUM

; ---------------------------------------------------------------------------
; Kernel stack (16KB, allocated in BSS)
; ---------------------------------------------------------------------------
section .bss
align 16
stack_bottom:
    resb 16384              ; 16KB of stack space
stack_top:

; Tell the linker this object doesn't need an executable stack.
section .note.GNU-stack noalloc noexec nowrite progbits

; ---------------------------------------------------------------------------
; _start — the kernel entry point
; ---------------------------------------------------------------------------
section .text
global _start
extern kernel_main

_start:
    mov esp, stack_top      ; point stack to the top of our 16KB BSS block
    push ebx                ; arg2: Multiboot info struct pointer
    push eax                ; arg1: Multiboot magic number
    call kernel_main        ; enter Rust code
    add esp, 8              ; clean up arguments (won't reach here)
.hang:
    cli                     ; disable interrupts
    hlt                     ; halt the CPU
    jmp .hang               ; loop in case of spurious wake-up (NMI)

; ---------------------------------------------------------------------------
; gdt_flush — Load a new GDT and reload all segment registers.
;
; Called from Rust as: gdt_flush(gdt_pointer_addr: u32)
;
; Selector 0x08 = GDT entry 1 (kernel code segment)
; Selector 0x10 = GDT entry 2 (kernel data segment)
; ---------------------------------------------------------------------------
global gdt_flush
gdt_flush:
    mov eax, [esp+4]       ; first argument: address of gdt_ptr struct
    lgdt [eax]             ; load the GDT register
    mov ax, 0x10           ; 0x10 = kernel data segment selector
    mov ds, ax             ; reload all data segment registers
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax
    jmp 0x08:.flush        ; far jump to 0x08 (code segment) to reload CS
.flush:
    ret

; ---------------------------------------------------------------------------
; context_switch -- Switch between two kernel threads.
;
; Rust prototype: extern "C" fn context_switch(old_esp: *mut u32, new_esp: u32)
;
; Saves callee-saved registers (ebx, esi, edi, ebp) on the current stack,
; stores the current ESP into *old_esp, then loads new_esp and restores the
; callee-saved registers from the new stack.
; ---------------------------------------------------------------------------
global context_switch
context_switch:
    mov eax, [esp + 4]    ; eax = pointer to save current ESP (*old_esp)
    mov edx, [esp + 8]    ; edx = new ESP to switch to

    ; Save callee-saved registers on the current (old) stack
    push ebx
    push esi
    push edi
    push ebp

    ; Save current stack pointer
    mov [eax], esp

    ; Switch to the new stack
    mov esp, edx

    ; Restore callee-saved registers from the new stack
    pop ebp
    pop edi
    pop esi
    pop ebx

    ret                    ; return into the new thread
