// idt.rs -- Interrupt Descriptor Table setup and interrupt dispatch for x86_64.
//
// The IDT tells the CPU which function to call for each of the 256 possible
// interrupt vectors. This module:
//   - Builds the 256-entry IDT and loads it with lidt
//   - Provides the common interrupt handler (isr_handler) called from the
//     assembly stubs in isr.s
//   - Manages a table of registered handler callbacks so other modules (like
//     the keyboard driver) can install their own interrupt handlers
//
// In 64-bit long mode each IDT entry is 16 bytes (not 8) because the handler
// address is 64 bits wide and there is an IST field.

use core::arch::asm;
use core::mem::size_of;

// Compiled-code range constants (rc module is currently disabled).
const CC_LOAD_BASE: u64 = 0x00A0_0000;
const CC_CODE_MAX: u64 = 65536;

/// Register state pushed onto the stack by isr_common_stub in isr.s.
///
/// Field order must exactly match the order values appear on the stack
/// (from low address to high address):
///   isr_common_stub pushes: r15, r14, r13, r12, r11, r10, r9, r8,
///                           rbp, rdi, rsi, rdx, rcx, rbx, rax
///   ISR stub pushes:        int_no, err_code
///   CPU pushes:             rip, cs, rflags, rsp, ss
#[repr(C)]
pub struct Registers {
    // Pushed by isr_common_stub (last pushed = lowest address = first field)
    pub r15: u64, pub r14: u64, pub r13: u64, pub r12: u64,
    pub r11: u64, pub r10: u64, pub r9: u64, pub r8: u64,
    pub rbp: u64, pub rdi: u64, pub rsi: u64, pub rdx: u64,
    pub rcx: u64, pub rbx: u64, pub rax: u64,
    // Pushed by ISR stub
    pub int_no: u64, pub err_code: u64,
    // Pushed by CPU on interrupt
    pub rip: u64, pub cs: u64, pub rflags: u64, pub rsp: u64, pub ss: u64,
}

/// 64-bit IDT entry (16 bytes). The handler address is split into three parts.
///
/// type_attr = 0x8E means:
///   bit 7    = 1 (present)
///   bits 6-5 = 00 (ring 0 = kernel only)
///   bit 4    = 0 (system segment)
///   bits 3-0 = 1110 (64-bit interrupt gate)
#[repr(C, packed)]
struct IdtEntry {
    offset_low:  u16,   // handler address bits 0-15
    selector:    u16,   // code segment selector (0x08 = kernel code)
    ist:         u8,    // interrupt stack table offset (0 = not used)
    type_attr:   u8,    // type and attributes
    offset_mid:  u16,   // handler address bits 16-31
    offset_high: u32,   // handler address bits 32-63
    reserved:    u32,   // must be zero
}

/// Pointer structure loaded by the lidt instruction.
/// In 64-bit mode the base field is 8 bytes.
#[repr(C, packed)]
struct IdtPtr {
    limit: u16,   // total size of IDT in bytes, minus 1
    base:  u64,   // linear address of the IDT array
}

static mut IDT: [IdtEntry; 256] = {
    const EMPTY: IdtEntry = IdtEntry {
        offset_low: 0, selector: 0, ist: 0, type_attr: 0,
        offset_mid: 0, offset_high: 0, reserved: 0,
    };
    [EMPTY; 256]
};

static mut IDTP: IdtPtr = IdtPtr { limit: 0, base: 0 };

/// Handler callback table -- one slot per interrupt vector.
static mut HANDLERS: [Option<fn(*mut Registers)>; 256] = [None; 256];

/// Human-readable exception names for vectors 0-31.
static EXCEPTION_NAMES: [&[u8]; 32] = [
    b"Division By Zero", b"Debug", b"Non Maskable Interrupt", b"Breakpoint",
    b"Overflow", b"Bound Range Exceeded", b"Invalid Opcode", b"Device Not Available",
    b"Double Fault", b"Coprocessor Segment Overrun", b"Invalid TSS",
    b"Segment Not Present", b"Stack-Segment Fault", b"General Protection Fault",
    b"Page Fault", b"Reserved", b"x87 FP Exception", b"Alignment Check",
    b"Machine Check", b"SIMD FP Exception", b"Virtualization Exception",
    b"Control Protection Exception", b"Reserved", b"Reserved", b"Reserved",
    b"Reserved", b"Reserved", b"Reserved", b"Reserved", b"Reserved",
    b"Security Exception", b"Reserved",
];

// CPU exception stubs (vectors 0-31), defined in isr.s
unsafe extern "C" {
    fn isr0();  fn isr1();  fn isr2();  fn isr3();
    fn isr4();  fn isr5();  fn isr6();  fn isr7();
    fn isr8();  fn isr9();  fn isr10(); fn isr11();
    fn isr12(); fn isr13(); fn isr14(); fn isr15();
    fn isr16(); fn isr17(); fn isr18(); fn isr19();
    fn isr20(); fn isr21(); fn isr22(); fn isr23();
    fn isr24(); fn isr25(); fn isr26(); fn isr27();
    fn isr28(); fn isr29(); fn isr30(); fn isr31();
}

// Hardware IRQ stubs (vectors 32-47), defined in isr.s
unsafe extern "C" {
    fn irq0();  fn irq1();  fn irq2();  fn irq3();
    fn irq4();  fn irq5();  fn irq6();  fn irq7();
    fn irq8();  fn irq9();  fn irq10(); fn irq11();
    fn irq12(); fn irq13(); fn irq14(); fn irq15();
}

// Syscall stub (vector 128 = int 0x80), defined in isr.s
unsafe extern "C" {
    fn isr128();
}

/// Install the int 0x80 syscall gate in the IDT.
pub unsafe fn install_isr128() {
    idt_set_gate(128, isr128 as u64);
}

/// Fill one IDT entry with a 64-bit handler address.
unsafe fn idt_set_gate(num: u8, handler: u64) {
    let i = num as usize;
    IDT[i].offset_low  = (handler & 0xFFFF) as u16;
    IDT[i].offset_mid  = ((handler >> 16) & 0xFFFF) as u16;
    IDT[i].offset_high = ((handler >> 32) & 0xFFFFFFFF) as u32;
    IDT[i].selector    = 0x08;   // kernel code segment
    IDT[i].ist         = 0;      // no IST
    IDT[i].type_attr   = 0x8E;   // present, ring 0, 64-bit interrupt gate
    IDT[i].reserved    = 0;
}

/// Register a callback for a specific interrupt vector.
pub fn register_handler(n: u8, handler: fn(*mut Registers)) {
    unsafe {
        HANDLERS[n as usize] = Some(handler);
    }
}

/// Format a u64 as a 16-digit hex string into a buffer at the given offset.
fn format_hex(buf: &mut [u8], offset: usize, val: u64) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    buf[offset]     = b'0';
    buf[offset + 1] = b'x';
    let mut k: u64 = 0;
    while k < 16 {
        buf[offset + 2 + k as usize] = HEX[((val >> ((15 - k) * 4)) & 0xF) as usize];
        k += 1;
    }
}

/// Common interrupt handler -- called from isr_common_stub in isr.s.
///
/// For CPU exceptions (vectors 0-31): print the exception name and halt,
/// or call thread_exit if the fault is in compiled-program code.
/// For hardware IRQs (vectors 32-47): call the registered handler, then
/// send End-Of-Interrupt to the PIC.
#[unsafe(no_mangle)]
pub extern "C" fn isr_handler(regs: *mut Registers) {
    unsafe {
        let int_no = (*regs).int_no;

        if int_no < 32 {
            let fault_rip = (*regs).rip;

            // If the fault occurred inside compiled-program code, kill only
            // the current thread instead of halting the whole OS.
            if fault_rip >= CC_LOAD_BASE
                && fault_rip < CC_LOAD_BASE + CC_CODE_MAX * 2
            {
                let mut rip_str = [0u8; 19];
                format_hex(&mut rip_str, 0, fault_rip);
                rip_str[18] = 0;

                crate::vga::puts(b"\n!!! ");
                crate::serial::puts(b"\n!!! ");
                crate::vga::write_bytes(EXCEPTION_NAMES[int_no as usize]);
                crate::serial::puts(EXCEPTION_NAMES[int_no as usize]);
                crate::vga::puts(b" at RIP=");
                crate::serial::puts(b" at RIP=");
                crate::vga::write_bytes(&rip_str[..18]);
                crate::serial::puts(&rip_str[..18]);

                if int_no == 14 {
                    let cr2: u64;
                    asm!("mov {0}, cr2", out(reg) cr2);
                    let mut cr2_str = [0u8; 19];
                    format_hex(&mut cr2_str, 0, cr2);
                    cr2_str[18] = 0;
                    crate::vga::puts(b" CR2=");
                    crate::serial::puts(b" CR2=");
                    crate::vga::write_bytes(&cr2_str[..18]);
                    crate::serial::puts(&cr2_str[..18]);
                }

                crate::vga::puts(b" in compiled code !!!\n");
                crate::serial::puts(b" in compiled code !!!\n");

                crate::thread::exit();  // kills current thread, scheduler picks next
                return;         // never reached
            }

            // Kernel exception -- print diagnostic and halt.
            crate::vga::puts(b"\n!!! EXCEPTION: ");
            crate::vga::write_bytes(EXCEPTION_NAMES[int_no as usize]);

            if int_no == 14 {
                let cr2: u64;
                asm!("mov {0}, cr2", out(reg) cr2);
                crate::vga::puts(b" at 0x");
                let mut k: u64 = 15;
                loop {
                    const HEX: &[u8; 16] = b"0123456789ABCDEF";
                    crate::vga::putchar(HEX[((cr2 >> (k * 4)) & 0xF) as usize]);
                    if k == 0 { break; }
                    k -= 1;
                }
            }

            crate::vga::puts(b" !!!\nSystem halted.");
            loop {
                asm!("cli", "hlt");
            }
        }

        // Call registered handler for this vector (if any).
        if let Some(handler) = HANDLERS[int_no as usize] {
            handler(regs);
        }

        // Acknowledge the interrupt so the PIC will deliver the next one.
        if int_no >= 32 && int_no < 48 {
            crate::pic::send_eoi((int_no - 32) as u8);
        }
    }
}

/// Build and load the IDT.
/// Installs handlers for all 32 CPU exceptions and 16 hardware IRQs.
pub unsafe fn init() {
    IDTP.limit = (size_of::<[IdtEntry; 256]>() - 1) as u16;
    IDTP.base  = &raw const IDT as u64;

    // Zero out all entries and handlers
    {
        let p = &raw mut IDT as *mut u8;
        let mut i = 0;
        while i < size_of::<[IdtEntry; 256]>() {
            *p.add(i) = 0;
            i += 1;
        }
    }
    {
        let mut i = 0;
        while i < 256 {
            HANDLERS[i] = None;
            i += 1;
        }
    }

    // CPU exceptions (vectors 0-31)
    idt_set_gate(0,  isr0  as u64);
    idt_set_gate(1,  isr1  as u64);
    idt_set_gate(2,  isr2  as u64);
    idt_set_gate(3,  isr3  as u64);
    idt_set_gate(4,  isr4  as u64);
    idt_set_gate(5,  isr5  as u64);
    idt_set_gate(6,  isr6  as u64);
    idt_set_gate(7,  isr7  as u64);
    idt_set_gate(8,  isr8  as u64);
    idt_set_gate(9,  isr9  as u64);
    idt_set_gate(10, isr10 as u64);
    idt_set_gate(11, isr11 as u64);
    idt_set_gate(12, isr12 as u64);
    idt_set_gate(13, isr13 as u64);
    idt_set_gate(14, isr14 as u64);
    idt_set_gate(15, isr15 as u64);
    idt_set_gate(16, isr16 as u64);
    idt_set_gate(17, isr17 as u64);
    idt_set_gate(18, isr18 as u64);
    idt_set_gate(19, isr19 as u64);
    idt_set_gate(20, isr20 as u64);
    idt_set_gate(21, isr21 as u64);
    idt_set_gate(22, isr22 as u64);
    idt_set_gate(23, isr23 as u64);
    idt_set_gate(24, isr24 as u64);
    idt_set_gate(25, isr25 as u64);
    idt_set_gate(26, isr26 as u64);
    idt_set_gate(27, isr27 as u64);
    idt_set_gate(28, isr28 as u64);
    idt_set_gate(29, isr29 as u64);
    idt_set_gate(30, isr30 as u64);
    idt_set_gate(31, isr31 as u64);

    // Hardware IRQs (vectors 32-47)
    idt_set_gate(32, irq0  as u64);
    idt_set_gate(33, irq1  as u64);
    idt_set_gate(34, irq2  as u64);
    idt_set_gate(35, irq3  as u64);
    idt_set_gate(36, irq4  as u64);
    idt_set_gate(37, irq5  as u64);
    idt_set_gate(38, irq6  as u64);
    idt_set_gate(39, irq7  as u64);
    idt_set_gate(40, irq8  as u64);
    idt_set_gate(41, irq9  as u64);
    idt_set_gate(42, irq10 as u64);
    idt_set_gate(43, irq11 as u64);
    idt_set_gate(44, irq12 as u64);
    idt_set_gate(45, irq13 as u64);
    idt_set_gate(46, irq14 as u64);
    idt_set_gate(47, irq15 as u64);

    // Load the IDT register -- CPU will now use our interrupt table.
    asm!("lidt [{}]", in(reg) &raw const IDTP, options(nostack));
}
