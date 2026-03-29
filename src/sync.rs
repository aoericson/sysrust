// sync.rs -- Synchronization primitives for kernel threads.
//
// Single-core x86: disabling interrupts is sufficient to prevent
// preemption by the timer. No atomic test-and-set is required.
//
// Spinlocks save/restore EFLAGS so that nested lock/unlock pairs
// work correctly and interrupt-context callers (e.g. IRQ handlers)
// don't accidentally re-enable interrupts on unlock.
//
// Mutexes yield the CPU on contention rather than spinning, making
// them suitable for longer critical sections. They must NOT be used
// from interrupt context (IRQ handlers cannot yield).

use core::arch::asm;
use crate::thread;

/// Spinlock: disables interrupts to prevent preemption.
pub struct Spinlock {
    pub locked: u32,
    pub saved_flags: u32,
}

impl Spinlock {
    /// Create a new unlocked spinlock.
    pub const fn new() -> Self {
        Spinlock {
            locked: 0,
            saved_flags: 0,
        }
    }

    /// Acquire the spinlock, disabling interrupts.
    pub unsafe fn lock(&mut self) {
        let flags: u32;
        asm!(
            "pushfd",
            "pop {0:e}",
            "cli",
            out(reg) flags,
            options(nostack),
        );
        self.saved_flags = flags;
        self.locked = 1;
    }

    /// Release the spinlock, restoring the saved interrupt state.
    pub unsafe fn unlock(&mut self) {
        let flags = self.saved_flags;
        self.locked = 0;
        asm!(
            "push {0:e}",
            "popfd",
            in(reg) flags,
            options(nostack),
        );
    }
}

/// Mutex: sleeps if contended instead of spinning.
pub struct Mutex {
    pub locked: u32,
    pub owner: i32,
}

impl Mutex {
    pub const fn new() -> Self {
        Mutex {
            locked: 0,
            owner: -1,
        }
    }

    /// Acquire the mutex, yielding the CPU while contended.
    pub unsafe fn lock(&mut self) {
        loop {
            let flags: u32;
            asm!(
                "pushfd",
                "pop {0:e}",
                "cli",
                out(reg) flags,
                options(nostack),
            );

            if self.locked == 0 || self.owner == thread::get_current() {
                self.locked = 1;
                self.owner = thread::get_current();
                asm!(
                    "push {0:e}",
                    "popfd",
                    in(reg) flags,
                    options(nostack),
                );
                return;
            }

            asm!(
                "push {0:e}",
                "popfd",
                in(reg) flags,
                options(nostack),
            );
            thread::yield_thread();
        }
    }

    /// Release the mutex.
    pub unsafe fn unlock(&mut self) {
        self.owner = -1;
        self.locked = 0;
    }
}
