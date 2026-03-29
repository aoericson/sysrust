// timer.rs -- PIT (Programmable Interval Timer) driver.
//
// Programs PIT channel 0 to fire IRQ0 at ~100 Hz (every 10ms).
// Maintains a monotonic tick counter used for timeouts and delays.
//
// The PIT oscillator runs at 1,193,182 Hz. Dividing by 11932 gives
// approximately 100 interrupts per second.

use crate::idt;
use crate::io::outb;
use crate::pic;
use crate::thread;
use core::arch::asm;

const PIT_CHANNEL0: u16 = 0x40;
const PIT_CMD: u16 = 0x43;
const PIT_DIVISOR: u16 = 11932; // 1193182 / 100 Hz

static mut TICK_COUNT: u32 = 0;

/// IRQ0 handler -- called every ~10ms by the PIT.
///
/// Increments the tick counter and triggers preemptive scheduling
/// every 10 ticks (100ms time slice).
fn timer_handler(_regs: *mut idt::Registers) {
    unsafe {
        // volatile write to ensure the compiler does not optimize away
        core::ptr::write_volatile(&raw mut TICK_COUNT, TICK_COUNT.wrapping_add(1));

        // Preemptive scheduling: yield every 10 ticks (100ms time slice)
        let ticks = core::ptr::read_volatile(&raw const TICK_COUNT);
        if ticks % 10 == 0 {
            thread::yield_thread();
        }
    }
}

/// Return the current tick count (increments ~100 times per second).
pub fn get_ticks() -> u32 {
    unsafe { core::ptr::read_volatile(&raw const TICK_COUNT) }
}

/// Busy-wait for the specified number of ticks (~10ms each at 100Hz).
///
/// Uses elapsed comparison to avoid overflow when tick_count wraps.
pub fn wait(ticks: u32) {
    let start = get_ticks();
    while get_ticks().wrapping_sub(start) < ticks {
        unsafe {
            asm!("hlt");
        }
    }
}

/// Initialize the PIT and register the IRQ0 handler.
///
/// PIT command byte 0x36 means:
///   Channel 0, access mode lobyte/hibyte, mode 3 (square wave), binary.
pub fn init() {
    unsafe {
        core::ptr::write_volatile(&raw mut TICK_COUNT, 0);

        // Register IRQ0 handler (vector 32)
        idt::register_handler(32, timer_handler);

        // Program PIT channel 0
        outb(PIT_CMD, 0x36);
        outb(PIT_CHANNEL0, (PIT_DIVISOR & 0xFF) as u8); // low byte
        outb(PIT_CHANNEL0, ((PIT_DIVISOR >> 8) & 0xFF) as u8); // high byte
    }
}
