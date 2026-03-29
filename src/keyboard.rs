// keyboard.rs -- PS/2 keyboard driver.
//
// When a key is pressed or released, the keyboard controller fires IRQ1.
// Our handler reads the scancode from port 0x60, translates it to ASCII
// using a lookup table, and pushes it into a ring buffer.
//
// The shell reads from this buffer via getchar(), which blocks (using hlt)
// until a character is available.
//
// Scancodes: We use PS/2 Scan Code Set 1 (the BIOS default). Each key has
// a "make" code (press) and a "break" code (release = make | 0x80).
//
// Limitations:
//   - Only handles single-byte scancodes (no extended 0xE0 prefix keys)
//   - No Caps Lock, Ctrl, or Alt support

use crate::idt;
use crate::io::inb;
use crate::pic;
use core::arch::asm;

const KEYBOARD_DATA_PORT: u16 = 0x60;
const BUFFER_SIZE: usize = 256;

/// Scancode-to-ASCII lookup table (Scan Code Set 1, US QWERTY layout).
/// Index = scancode, value = ASCII character (0 = no mapping / ignored).
static SCANCODE_ASCII: [u8; 58] = [
    0, 0, b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', b'0', b'-', b'=', b'\x08',
    b'\t', b'q', b'w', b'e', b'r', b't', b'y', b'u', b'i', b'o', b'p', b'[', b']', b'\n',
    0, b'a', b's', b'd', b'f', b'g', b'h', b'j', b'k', b'l', b';', b'\'', b'`',
    0, b'\\', b'z', b'x', b'c', b'v', b'b', b'n', b'm', b',', b'.', b'/', 0,
    b'*', 0, b' ',
];

/// Shifted scancode-to-ASCII lookup table.
static SCANCODE_SHIFT: [u8; 58] = [
    0, 0, b'!', b'@', b'#', b'$', b'%', b'^', b'&', b'*', b'(', b')', b'_', b'+', b'\x08',
    b'\t', b'Q', b'W', b'E', b'R', b'T', b'Y', b'U', b'I', b'O', b'P', b'{', b'}', b'\n',
    0, b'A', b'S', b'D', b'F', b'G', b'H', b'J', b'K', b'L', b':', b'"', b'~',
    0, b'|', b'Z', b'X', b'C', b'V', b'B', b'N', b'M', b'<', b'>', b'?', 0,
    b'*', 0, b' ',
];

const SCANCODE_TABLE_SIZE: usize = 58;

/// Ring buffer for keystrokes.
///
/// The IRQ handler writes to buf_head. The main loop reads from buf_tail.
/// buf_head and buf_tail are volatile because they are accessed from both
/// interrupt context and the main loop.
static mut BUFFER: [u8; BUFFER_SIZE] = [0; BUFFER_SIZE];
static mut BUF_HEAD: i32 = 0; // next write position (written by IRQ handler)
static mut BUF_TAIL: i32 = 0; // next read position (read by main loop)
static mut SHIFT_HELD: i32 = 0; // 1 if either shift key is currently pressed

/// IRQ1 handler -- called from the IDT when the keyboard fires.
/// Runs in interrupt context (interrupts are disabled).
fn keyboard_handler(_regs: *mut idt::Registers) {
    unsafe {
        let scancode = inb(KEYBOARD_DATA_PORT);

        // Track shift key state
        if scancode == 0x2A || scancode == 0x36 {
            SHIFT_HELD = 1;
            return;
        }
        if scancode == 0xAA || scancode == 0xB6 {
            SHIFT_HELD = 0;
            return;
        }

        // Ignore all other key releases (bit 7 set = break code)
        if scancode & 0x80 != 0 {
            return;
        }

        // Ignore scancodes outside our table
        if (scancode as usize) >= SCANCODE_TABLE_SIZE {
            return;
        }

        // Translate scancode to ASCII
        let c = if SHIFT_HELD != 0 {
            SCANCODE_SHIFT[scancode as usize]
        } else {
            SCANCODE_ASCII[scancode as usize]
        };

        // Ignore unmapped keys (0 in the table)
        if c == 0 {
            return;
        }

        // Push to ring buffer, dropping the keystroke if the buffer is full
        let head = core::ptr::read_volatile(&raw const BUF_HEAD);
        let tail = core::ptr::read_volatile(&raw const BUF_TAIL);
        let next = (head + 1) % BUFFER_SIZE as i32;
        if next != tail {
            BUFFER[head as usize] = c;
            core::ptr::write_volatile(&raw mut BUF_HEAD, next);
        }
    }
}

/// Initialize the keyboard driver and register the IRQ1 handler.
pub fn init() {
    unsafe {
        core::ptr::write_volatile(&raw mut BUF_HEAD, 0);
        core::ptr::write_volatile(&raw mut BUF_TAIL, 0);
        SHIFT_HELD = 0;
        idt::register_handler(33, keyboard_handler); // IRQ1 = interrupt vector 33
    }
}

/// Read one character from the keyboard (blocking).
///
/// Spins on hlt until the ring buffer has data. hlt puts the CPU to sleep
/// until the next interrupt, so this doesn't burn CPU cycles while waiting.
pub fn getchar() -> u8 {
    unsafe {
        while core::ptr::read_volatile(&raw const BUF_HEAD)
            == core::ptr::read_volatile(&raw const BUF_TAIL)
        {
            asm!("hlt"); // sleep until next interrupt
        }
        let tail = core::ptr::read_volatile(&raw const BUF_TAIL);
        let c = BUFFER[tail as usize];
        core::ptr::write_volatile(&raw mut BUF_TAIL, (tail + 1) % BUFFER_SIZE as i32);
        c
    }
}

/// Non-blocking read: returns Some(char) if data is available, None otherwise.
pub fn try_getchar() -> Option<u8> {
    unsafe {
        if core::ptr::read_volatile(&raw const BUF_HEAD)
            == core::ptr::read_volatile(&raw const BUF_TAIL)
        {
            return None;
        }
        let tail = core::ptr::read_volatile(&raw const BUF_TAIL);
        let c = BUFFER[tail as usize];
        core::ptr::write_volatile(&raw mut BUF_TAIL, (tail + 1) % BUFFER_SIZE as i32);
        Some(c)
    }
}

/// Non-blocking check if the keyboard ring buffer has data.
pub fn data_ready() -> bool {
    unsafe {
        core::ptr::read_volatile(&raw const BUF_HEAD)
            != core::ptr::read_volatile(&raw const BUF_TAIL)
    }
}
