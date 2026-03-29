; isr.s -- Interrupt Service Routine assembly stubs.
;
; When the CPU receives an interrupt, it pushes EFLAGS, CS, and EIP onto the
; stack (and sometimes an error code). These stubs bridge that gap:
;
;   1. Push a uniform stack frame (error code + interrupt number)
;   2. Save all registers with pusha
;   3. Call the Rust handler (isr_handler in idt.rs)
;   4. Restore registers and iret back to interrupted code

; ---- Macro: exception WITHOUT CPU-pushed error code ----
%macro ISR_NOERRCODE 1
global isr%1
isr%1:
    push dword 0          ; dummy error code
    push dword %1         ; interrupt number
    jmp isr_common_stub
%endmacro

; ---- Macro: exception WITH CPU-pushed error code ----
%macro ISR_ERRCODE 1
global isr%1
isr%1:
    push dword %1         ; interrupt number (error code already on stack)
    jmp isr_common_stub
%endmacro

; ---- Macro: hardware IRQ ----
%macro IRQ 2
global irq%1
irq%1:
    push dword 0          ; dummy error code
    push dword %2         ; interrupt vector number (32 + IRQ number)
    jmp isr_common_stub
%endmacro

; ---- CPU exception stubs (vectors 0-31) ----
ISR_NOERRCODE 0   ; #DE - Division By Zero
ISR_NOERRCODE 1   ; #DB - Debug
ISR_NOERRCODE 2   ;       Non-Maskable Interrupt
ISR_NOERRCODE 3   ; #BP - Breakpoint
ISR_NOERRCODE 4   ; #OF - Overflow
ISR_NOERRCODE 5   ; #BR - Bound Range Exceeded
ISR_NOERRCODE 6   ; #UD - Invalid Opcode
ISR_NOERRCODE 7   ; #NM - Device Not Available
ISR_ERRCODE   8   ; #DF - Double Fault
ISR_NOERRCODE 9   ;       Coprocessor Segment Overrun
ISR_ERRCODE   10  ; #TS - Invalid TSS
ISR_ERRCODE   11  ; #NP - Segment Not Present
ISR_ERRCODE   12  ; #SS - Stack-Segment Fault
ISR_ERRCODE   13  ; #GP - General Protection Fault
ISR_ERRCODE   14  ; #PF - Page Fault
ISR_NOERRCODE 15  ;       Reserved
ISR_NOERRCODE 16  ; #MF - x87 Floating-Point Exception
ISR_ERRCODE   17  ; #AC - Alignment Check
ISR_NOERRCODE 18  ; #MC - Machine Check
ISR_NOERRCODE 19  ; #XM - SIMD Floating-Point Exception
ISR_NOERRCODE 20  ; #VE - Virtualization Exception
ISR_NOERRCODE 21  ; #CP - Control Protection Exception
ISR_NOERRCODE 22  ;       Reserved
ISR_NOERRCODE 23  ;       Reserved
ISR_NOERRCODE 24  ;       Reserved
ISR_NOERRCODE 25  ;       Reserved
ISR_NOERRCODE 26  ;       Reserved
ISR_NOERRCODE 27  ;       Reserved
ISR_NOERRCODE 28  ;       Reserved
ISR_NOERRCODE 29  ;       Reserved
ISR_ERRCODE   30  ; #SX - Security Exception
ISR_NOERRCODE 31  ;       Reserved

; ---- Hardware IRQ stubs (vectors 32-47) ----
IRQ 0,  32  ; PIT timer
IRQ 1,  33  ; Keyboard
IRQ 2,  34  ; Cascade
IRQ 3,  35  ; COM2 / COM4
IRQ 4,  36  ; COM1 / COM3
IRQ 5,  37  ; LPT2 / sound card
IRQ 6,  38  ; Floppy disk controller
IRQ 7,  39  ; LPT1 / spurious
IRQ 8,  40  ; RTC
IRQ 9,  41  ; ACPI / available
IRQ 10, 42  ; Available
IRQ 11, 43  ; Available
IRQ 12, 44  ; PS/2 mouse
IRQ 13, 45  ; FPU / coprocessor
IRQ 14, 46  ; Primary ATA
IRQ 15, 47  ; Secondary ATA

; ---------------------------------------------------------------------------
; isr_common_stub -- Common entry point for all interrupt stubs.
; ---------------------------------------------------------------------------
extern isr_handler

isr_common_stub:
    pusha               ; save edi, esi, ebp, esp, ebx, edx, ecx, eax
    mov eax, esp        ; eax = address of saved registers (Registers*)
    push eax            ; pass as argument to isr_handler
    call isr_handler    ; dispatch to the Rust interrupt handler
    add esp, 4          ; pop the argument
    popa                ; restore all general-purpose registers
    add esp, 8          ; discard int_no and err_code from the stack
    iret                ; return from interrupt

; Tell the linker this object doesn't need an executable stack
section .note.GNU-stack noalloc noexec nowrite progbits
