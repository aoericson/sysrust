; isr.s — 64-bit ISR/IRQ stubs

%macro ISR_NOERRCODE 1
global isr%1
isr%1:
    push qword 0          ; dummy error code
    push qword %1         ; interrupt number
    jmp isr_common_stub
%endmacro

%macro ISR_ERRCODE 1
global isr%1
isr%1:
    push qword %1         ; interrupt number (error code already pushed by CPU)
    jmp isr_common_stub
%endmacro

%macro IRQ 2
global irq%1
irq%1:
    push qword 0
    push qword %2
    jmp isr_common_stub
%endmacro

; CPU exceptions 0-31
ISR_NOERRCODE 0
ISR_NOERRCODE 1
ISR_NOERRCODE 2
ISR_NOERRCODE 3
ISR_NOERRCODE 4
ISR_NOERRCODE 5
ISR_NOERRCODE 6
ISR_NOERRCODE 7
ISR_ERRCODE   8
ISR_NOERRCODE 9
ISR_ERRCODE   10
ISR_ERRCODE   11
ISR_ERRCODE   12
ISR_ERRCODE   13
ISR_ERRCODE   14
ISR_NOERRCODE 15
ISR_NOERRCODE 16
ISR_ERRCODE   17
ISR_NOERRCODE 18
ISR_NOERRCODE 19
ISR_NOERRCODE 20
ISR_NOERRCODE 21
ISR_NOERRCODE 22
ISR_NOERRCODE 23
ISR_NOERRCODE 24
ISR_NOERRCODE 25
ISR_NOERRCODE 26
ISR_NOERRCODE 27
ISR_NOERRCODE 28
ISR_NOERRCODE 29
ISR_ERRCODE   30
ISR_NOERRCODE 31

; Hardware IRQs 32-47
IRQ 0,  32
IRQ 1,  33
IRQ 2,  34
IRQ 3,  35
IRQ 4,  36
IRQ 5,  37
IRQ 6,  38
IRQ 7,  39
IRQ 8,  40
IRQ 9,  41
IRQ 10, 42
IRQ 11, 43
IRQ 12, 44
IRQ 13, 45
IRQ 14, 46
IRQ 15, 47

; Syscall (vector 128)
global isr128
isr128:
    push qword 0
    push qword 128
    jmp isr_common_stub

extern isr_handler

isr_common_stub:
    ; Save all general-purpose registers (no pusha on x86_64)
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push rbp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15

    ; First argument (RDI) = pointer to saved registers on stack
    mov rdi, rsp
    call isr_handler

    ; Restore all registers
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rbp
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rbx
    pop rax

    add rsp, 16          ; skip int_no and err_code
    iretq                ; 64-bit interrupt return

; ---------------------------------------------------------------------------
; syscall_entry -- Entry point for the `syscall` instruction (x86_64 Linux ABI).
;
; On entry by CPU: RCX = return RIP, R11 = saved RFLAGS, RIP = LSTAR
; Syscall args:    RAX = nr, RDI = a1, RSI = a2, RDX = a3, R10 = a4, R8 = a5, R9 = a6
; Return:          RAX = result, then SYSRETQ restores RIP from RCX, RFLAGS from R11
; ---------------------------------------------------------------------------
extern syscall_dispatch_x64

global syscall_entry
syscall_entry:
    ; On entry: RAX=nr, RDI=a1, RSI=a2, RDX=a3, R10=a4, R8=a5, R9=a6
    ;           RCX=return RIP (saved by CPU), R11=saved RFLAGS

    ; Save return state and callee-saved regs
    push rcx            ; return RIP
    push r11            ; saved RFLAGS
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15

    ; Rearrange from Linux ABI to System V ABI:
    ;   Linux:    rax=nr, rdi=a1, rsi=a2, rdx=a3, r10=a4, r8=a5, r9=a6
    ;   SysV:     rdi=nr, rsi=a1, rdx=a2, rcx=a3, r8=a4,  r9=a5
    ;
    ; Must be careful not to clobber values we still need.
    ; Order matters: move rdi last (it's both source a1 and dest nr).
    mov rcx, r10        ; a3: r10 -> rcx  (rcx was clobbered by CPU, free)
    ; r8 stays (a4->a4), r9 stays (a5->a5)
    ; rdx stays (a2->a2) -- WAIT: rdx is a3 in Linux, a2 in SysV
    ; Actually: Linux rsi=a2 -> SysV rsi=a1... NO.
    ;
    ; Let me be precise:
    ;   Linux arg1 = rdi  -> SysV arg1 = rsi
    ;   Linux arg2 = rsi  -> SysV arg2 = rdx
    ;   Linux arg3 = rdx  -> SysV arg3 = rcx  (done above via r10)
    ; WAIT: Linux a3 is rdx, SysV a3 is rcx. Linux a4 is r10, mapped to SysV a3=rcx? No.
    ;
    ; Correct mapping:
    ;   syscall_dispatch_x64(nr, a1, a2, a3, a4, a5)
    ;   SysV:  rdi=nr, rsi=a1, rdx=a2, rcx=a3, r8=a4, r9=a5
    ;   Linux: rax=nr, rdi=a1, rsi=a2, rdx=a3, r10=a4, r8=a5
    ;
    ;   nr:  rax -> rdi
    ;   a1:  rdi -> rsi   (rdi is also the source of nr's destination!)
    ;   a2:  rsi -> rdx   (rsi also needs to become a1... wait)
    ;
    ; This is a 3-way register shuffle. Use a temp:
    mov r15, rdx        ; save Linux a3 (rdx) in r15
    mov rdx, rsi        ; SysV a2 = Linux a2 (rsi -> rdx)
    mov rsi, rdi        ; SysV a1 = Linux a1 (rdi -> rsi)
    mov rdi, rax        ; SysV nr = Linux nr (rax -> rdi)
    mov rcx, r15        ; SysV a3 = Linux a3 (was rdx, saved in r15)
    ; r8 = a4 (stays -- Linux r10 maps to SysV r8? NO)
    ; Linux a4 = r10, SysV arg4 = r8. Need: mov r8_new = r10? But r8 has Linux a5.
    ; Linux: r10=a4, r8=a5, r9=a6
    ; SysV:  r8=a4,  r9=a5
    ; So: r8_new = r10 (Linux a4), r9_new = r8 (Linux a5)
    mov r15, r8         ; save Linux a5
    mov r8, r10         ; SysV a4 = Linux a4 (r10 -> r8)
    mov r9, r15         ; SysV a5 = Linux a5 (was r8, saved in r15)

    ; Align stack to 16 bytes
    mov rbp, rsp
    and rsp, -16

    call syscall_dispatch_x64

    mov rsp, rbp

    ; Restore callee-saved regs and return state
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx
    pop r11             ; RFLAGS for sysretq
    pop rcx             ; return RIP for sysretq

    ; Re-enable interrupts
    sti

    sysretq

section .note.GNU-stack noalloc noexec nowrite progbits
