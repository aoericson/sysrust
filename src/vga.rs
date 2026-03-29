// vga.rs -- VGA text-mode driver (80x25 / 80x50).
//
// The VGA text buffer is memory-mapped at physical address 0xB8000.
// Each on-screen character is 2 bytes: the ASCII value and an attribute
// byte (4-bit background color in the high nibble, 4-bit foreground in
// the low nibble).
//
// Writing to this memory immediately updates the display -- there are no
// system calls or framebuffer flips involved.
//
// The hardware cursor (the blinking underscore) is controlled separately
// via the VGA CRT controller registers at I/O ports 0x3D4/0x3D5.

use crate::io::outb;
use crate::serial;
use crate::sync::Spinlock;

/// VGA color constants.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Color {
    Black        = 0,
    Blue         = 1,
    Green        = 2,
    Cyan         = 3,
    Red          = 4,
    Magenta      = 5,
    Brown        = 6,
    LightGrey    = 7,
    DarkGrey     = 8,
    LightBlue    = 9,
    LightGreen   = 10,
    LightCyan    = 11,
    LightRed     = 12,
    LightMagenta = 13,
    Yellow       = 14,
    White        = 15,
}

const VGA_ADDR: usize = 0xB8000;

static mut VGA_BUFFER: *mut u16 = 0 as *mut u16;
static mut VGA_WIDTH: i32 = 80;
static mut VGA_HEIGHT: i32 = 25;
static mut CURSOR_ROW: i32 = 0;
static mut CURSOR_COL: i32 = 0;
static mut CURRENT_COLOR: u8 = 0x07; // (Black << 4) | LightGrey
static mut SERIAL_MIRROR: bool = false;

static mut VGA_LOCK: Spinlock = Spinlock::new();

#[inline]
unsafe fn lock() {
    VGA_LOCK.lock();
}

#[inline]
unsafe fn unlock() {
    VGA_LOCK.unlock();
}

/// Pack an ASCII character and color attribute into one 16-bit VGA entry.
#[inline]
fn vga_entry(c: u8, color: u8) -> u16 {
    (c as u16) | ((color as u16) << 8)
}

/// Update the hardware cursor position to match our software cursor.
/// The VGA CRT controller uses a 16-bit offset (row * width + col) written
/// in two 8-bit halves to register 14 (high byte) and 15 (low byte).
unsafe fn update_cursor() {
    let pos = (CURSOR_ROW * VGA_WIDTH + CURSOR_COL) as u16;
    outb(0x3D4, 14);                   // select register 14 (cursor high)
    outb(0x3D5, (pos >> 8) as u8);     // write high byte
    outb(0x3D4, 15);                   // select register 15 (cursor low)
    outb(0x3D5, (pos & 0xFF) as u8);   // write low byte
}

/// Scroll the screen up by one line if the cursor has moved past the bottom.
/// Copies rows 1..height to rows 0..height-1, then blanks the last row.
unsafe fn scroll() {
    if CURSOR_ROW < VGA_HEIGHT {
        return;
    }
    // Shift the entire buffer up by one row (overlapping regions -> copy carefully)
    let row_bytes = VGA_WIDTH as usize;
    let total = (VGA_HEIGHT as usize - 1) * row_bytes;
    // memmove forward (dest < src): copy from index row_bytes to index 0
    let buf = VGA_BUFFER;
    for i in 0..total {
        let val = core::ptr::read_volatile(buf.add(i + row_bytes));
        core::ptr::write_volatile(buf.add(i), val);
    }
    // Blank the last row with proper color attribute
    let last_row_start = (VGA_HEIGHT as usize - 1) * row_bytes;
    for i in 0..row_bytes {
        core::ptr::write_volatile(buf.add(last_row_start + i), vga_entry(b' ', CURRENT_COLOR));
    }
    CURSOR_ROW = VGA_HEIGHT - 1;
}

/// Unlocked putchar helper -- must be called with the VGA lock held.
unsafe fn putchar_unlocked(c: u8) {
    if c == b'\n' {
        CURSOR_COL = 0;
        CURSOR_ROW += 1;
    } else if c == b'\x08' {
        // backspace
        if CURSOR_COL > 0 {
            CURSOR_COL -= 1;
            let offset = (CURSOR_ROW * VGA_WIDTH + CURSOR_COL) as usize;
            core::ptr::write_volatile(VGA_BUFFER.add(offset), vga_entry(b' ', CURRENT_COLOR));
        }
    } else {
        let offset = (CURSOR_ROW * VGA_WIDTH + CURSOR_COL) as usize;
        core::ptr::write_volatile(VGA_BUFFER.add(offset), vga_entry(c, CURRENT_COLOR));
        CURSOR_COL += 1;
        if CURSOR_COL >= VGA_WIDTH {
            CURSOR_COL = 0;
            CURSOR_ROW += 1;
        }
    }
    scroll();
    update_cursor();
}

/// Initialize the VGA driver: clear screen, reset cursor, set default color.
pub unsafe fn init() {
    VGA_BUFFER = VGA_ADDR as *mut u16;
    CURSOR_ROW = 0;
    CURSOR_COL = 0;
    CURRENT_COLOR = ((Color::Black as u8) << 4) | (Color::LightGrey as u8);
    clear();
}

/// Switch to 80x50 text mode by changing the VGA font from 8x16 to 8x8.
///
/// Standard VGA mode 3 uses a 9x16 cell. With 400 vertical scan lines,
/// that gives 400/16 = 25 rows. Reprogramming the Maximum Scan Line
/// register to 7 (8 pixels per row) gives 400/8 = 50 rows.
pub unsafe fn set_80x50() {
    lock();

    // CRT Controller register 9: Maximum Scan Line
    // Set to 7 for 8-pixel-high characters (was 15 for 16-pixel)
    outb(0x3D4, 0x09);
    outb(0x3D5, 0x07);

    // CRT Controller register 10: Cursor Start
    // Set cursor to start at scan line 6 (thin cursor near bottom of cell)
    outb(0x3D4, 0x0A);
    outb(0x3D5, 0x06);

    // CRT Controller register 11: Cursor End
    // Set cursor to end at scan line 7
    outb(0x3D4, 0x0B);
    outb(0x3D5, 0x07);

    VGA_HEIGHT = 50;
    // Width stays 80 -- wider requires full VGA timing reprogramming

    // Clear the screen at the new dimensions
    let total = (VGA_WIDTH * VGA_HEIGHT) as usize;
    for i in 0..total {
        core::ptr::write_volatile(VGA_BUFFER.add(i), vga_entry(b' ', CURRENT_COLOR));
    }
    CURSOR_ROW = 0;
    CURSOR_COL = 0;
    update_cursor();

    unlock();
}

/// Clear the entire screen and reset the cursor to (0, 0).
pub unsafe fn clear() {
    lock();
    let total = (VGA_WIDTH * VGA_HEIGHT) as usize;
    for i in 0..total {
        core::ptr::write_volatile(VGA_BUFFER.add(i), vga_entry(b' ', CURRENT_COLOR));
    }
    CURSOR_ROW = 0;
    CURSOR_COL = 0;
    update_cursor();
    unlock();
}

/// Write a single character to the screen at the current cursor position.
/// Handles newline, backspace, line wrapping, and scrolling.
/// If serial mirroring is enabled, also writes to COM1.
pub unsafe fn putchar(c: u8) {
    lock();
    putchar_unlocked(c);
    if SERIAL_MIRROR {
        serial::putchar(c);
    }
    unlock();
}

/// Print a byte slice to the screen (stops at first null byte or end of slice).
/// Acquires the lock once for the entire string to prevent interleaving.
pub unsafe fn puts(s: &[u8]) {
    lock();
    for &c in s {
        if c == 0 {
            break;
        }
        putchar_unlocked(c);
        if SERIAL_MIRROR {
            serial::putchar(c);
        }
    }
    unlock();
}

/// Write a byte slice to the screen (all bytes, does not stop at null).
/// Used for printing Rust &str / &[u8] data that may contain interior nulls.
pub unsafe fn write_bytes(s: &[u8]) {
    lock();
    for &c in s {
        putchar_unlocked(c);
        if SERIAL_MIRROR {
            serial::putchar(c);
        }
    }
    unlock();
}

/// Change the current text color (affects subsequent writes).
pub unsafe fn set_color(fg: Color, bg: Color) {
    CURRENT_COLOR = ((bg as u8) << 4) | (fg as u8);
}

/// Enable or disable serial mirroring of all VGA output.
pub unsafe fn set_serial_mirror(enable: bool) {
    SERIAL_MIRROR = enable;
}

/// Query whether serial mirroring is currently enabled.
pub unsafe fn get_serial_mirror() -> bool {
    SERIAL_MIRROR
}
