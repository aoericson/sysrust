// thread.rs -- Kernel thread scheduler.
//
// Implements a simple round-robin scheduler for kernel-mode threads.
// context_switch() is written in assembly (boot/boot.s) and swaps the
// stack pointer between two threads, saving/restoring callee-saved regs.
//
// Thread 0 is the boot/shell thread; it uses the existing boot stack and
// is never freed. Additional threads get 4KB stacks from kmalloc.
//
// Preemptive scheduling: timer_handler() calls yield_thread() every
// 10 ticks (100ms time slice).

use crate::heap;
use core::arch::asm;
use core::ptr;

const THREAD_STACK_SIZE: usize = 4096; // 4KB per thread stack
pub const MAX_THREADS: usize = 32;

/// Thread states matching the C enum.
#[derive(Clone, Copy, PartialEq)]
#[repr(C)]
pub enum ThreadState {
    Ready,
    Running,
    Blocked,
    Dead,
}

/// Per-thread control block.
#[repr(C)]
pub struct Thread {
    pub id: u32,
    pub rsp: u64,
    pub state: ThreadState,
    pub stack_base: *mut u8, // base of kmalloc'd stack (null for thread 0)
}

/// Assembly routine in boot/boot.s -- swaps stack pointers and
/// saves/restores callee-saved registers (rbx, rbp, r12, r13, r14, r15).
unsafe extern "C" {
    fn context_switch(old_rsp: *mut u64, new_rsp: u64);
}

static mut THREADS: [Thread; MAX_THREADS] = {
    const DEAD_THREAD: Thread = Thread {
        id: 0,
        rsp: 0,
        state: ThreadState::Dead,
        stack_base: ptr::null_mut(),
    };
    [DEAD_THREAD; MAX_THREADS]
};

static mut CURRENT_THREAD: usize = 0; // index of the currently running thread
static mut NEXT_ID: u32 = 1; // next thread ID to assign

/// Deferred stack free: exit() records the stack to free here.
/// schedule() actually calls kfree AFTER context_switch has moved
/// execution to a different stack, avoiding use-after-free.
static mut PENDING_FREE_STACK: *mut u8 = ptr::null_mut();

/// Initialize the threading subsystem.
///
/// Marks the calling context (the boot/kernel thread) as thread 0.
/// Its stack is the boot stack set up in boot.s, so stack_base is NULL.
pub fn init() {
    unsafe {
        for i in 0..MAX_THREADS {
            THREADS[i].state = ThreadState::Dead;
        }

        THREADS[0].id = 0;
        THREADS[0].rsp = 0; // filled on first context switch
        THREADS[0].state = ThreadState::Running;
        THREADS[0].stack_base = ptr::null_mut(); // boot stack, not from kmalloc

        CURRENT_THREAD = 0;
        NEXT_ID = 1;
    }
}

/// Spawn a new kernel thread.
///
/// Allocates a 4KB stack and pre-builds a stack frame that looks as if the
/// thread had just called context_switch(). When the scheduler switches to
/// this thread for the first time, context_switch's `ret` will jump to
/// `entry`, and if entry() ever returns it falls through to exit().
///
/// Returns the thread ID on success, or -1 if the thread table is full.
pub fn create(entry: extern "C" fn()) -> i32 {
    unsafe {
        // Find a free slot
        let mut slot = MAX_THREADS;
        for i in 0..MAX_THREADS {
            if THREADS[i].state == ThreadState::Dead {
                slot = i;
                break;
            }
        }
        if slot == MAX_THREADS {
            return -1; // no room
        }

        let stack = heap::kmalloc(THREAD_STACK_SIZE) as *mut u8;
        if stack.is_null() {
            return -1;
        }

        // Zero the stack
        ptr::write_bytes(stack, 0, THREAD_STACK_SIZE);

        // Stack grows downward; sp starts at the top
        let mut sp = stack.add(THREAD_STACK_SIZE) as *mut u64;

        // Build the initial stack frame (addresses decrease as we push).
        // context_switch saves/restores in push rbx/rbp/r12/r13/r14/r15 order,
        // then restores with pop r15/r14/r13/r12/rbp/rbx (LIFO).
        // So the initial frame must have (from high to low address):
        //
        //   exit          <- return address if entry() returns
        //   entry         <- return address for context_switch's `ret`
        //   0             <- saved r15 (popped first by context_switch)
        //   0             <- saved r14
        //   0             <- saved r13
        //   0             <- saved r12
        //   0             <- saved rbp
        //   0             <- saved rbx (popped last, SP points here)
        sp = sp.offset(-1);
        *sp = exit as u64; // if entry() returns
        sp = sp.offset(-1);
        *sp = entry as u64; // context_switch ret target
        sp = sp.offset(-1);
        *sp = 0; // saved r15 (popped first)
        sp = sp.offset(-1);
        *sp = 0; // saved r14
        sp = sp.offset(-1);
        *sp = 0; // saved r13
        sp = sp.offset(-1);
        *sp = 0; // saved r12
        sp = sp.offset(-1);
        *sp = 0; // saved rbp
        sp = sp.offset(-1);
        *sp = 0; // saved rbx (popped last, at SP)

        let tid = NEXT_ID;
        NEXT_ID += 1;

        THREADS[slot].id = tid;
        THREADS[slot].rsp = sp as u64;
        THREADS[slot].state = ThreadState::Ready;
        THREADS[slot].stack_base = stack;

        tid as i32
    }
}

/// Pick the next READY thread and switch to it.
///
/// Round-robin scan starting one past the current thread. If no other
/// thread is ready, we just keep running the current one.
unsafe fn schedule() {
    // Free any stack deferred from a previous exit().
    // Safe now because we're on a different thread's stack.
    if !PENDING_FREE_STACK.is_null() {
        heap::kfree(PENDING_FREE_STACK);
        PENDING_FREE_STACK = ptr::null_mut();
    }

    let old = CURRENT_THREAD;

    // Round-robin: scan from current+1, wrapping around
    let mut next = old;
    for i in 1..=MAX_THREADS {
        let candidate = (old + i) % MAX_THREADS;
        if THREADS[candidate].state == ThreadState::Ready {
            next = candidate;
            break;
        }
    }

    if next == old {
        return; // no other thread to run
    }

    // Update states
    if THREADS[old].state == ThreadState::Running {
        THREADS[old].state = ThreadState::Ready;
    }

    THREADS[next].state = ThreadState::Running;
    CURRENT_THREAD = next;

    context_switch(&raw mut THREADS[old].rsp, THREADS[next].rsp);
}

/// Voluntarily give up the CPU.
///
/// Safe to call from both normal thread code and from the timer interrupt
/// handler. When called from the timer handler, interrupts are already
/// disabled by the interrupt gate; for voluntary yields we bracket the
/// switch with cli/sti to be safe.
pub fn yield_thread() {
    unsafe {
        // Save and restore the previous IF state via pushfq/popfq so
        // voluntary yields re-enable interrupts afterwards and
        // interrupt-context yields leave them disabled.
        let flags: u64;
        asm!("pushfq; pop {0}", out(reg) flags);
        asm!("cli");

        schedule();

        // Restore the interrupt flag to its previous state
        if flags & 0x200 != 0 {
            // bit 9 = IF
            asm!("sti");
        }
    }
}

/// Terminate the current thread.
///
/// Marks the thread DEAD and defers stack free. Never returns (yields to
/// another thread, which will never switch back).
pub extern "C" fn exit() {
    unsafe {
        asm!("cli");

        let cur = CURRENT_THREAD;
        THREADS[cur].state = ThreadState::Dead;

        // Defer stack free -- schedule() will kfree after context_switch
        // moves us off this stack. Freeing here would be use-after-free.
        if !THREADS[cur].stack_base.is_null() {
            PENDING_FREE_STACK = THREADS[cur].stack_base;
            THREADS[cur].stack_base = ptr::null_mut();
        }

        // Switch away; schedule() will find another READY thread
        schedule();

        // Should never reach here, but just in case: halt forever
        loop {
            asm!("hlt");
        }
    }
}

/// Return the ID of the currently running thread.
pub fn get_current() -> i32 {
    unsafe { THREADS[CURRENT_THREAD].id as i32 }
}

/// Check if a thread with the given ID is alive (not DEAD).
pub fn is_alive(id: i32) -> bool {
    unsafe {
        for i in 0..MAX_THREADS {
            if THREADS[i].state != ThreadState::Dead && THREADS[i].id == id as u32 {
                return true;
            }
        }
        false
    }
}

/// Return the total number of live (non-DEAD) threads.
pub fn get_count() -> i32 {
    unsafe {
        let mut count = 0;
        for i in 0..MAX_THREADS {
            if THREADS[i].state != ThreadState::Dead {
                count += 1;
            }
        }
        count
    }
}

/// Return a const pointer to the thread at the given slot index, or null.
pub fn get(index: i32) -> *const Thread {
    if index < 0 || index >= MAX_THREADS as i32 {
        return ptr::null();
    }
    unsafe { &raw const THREADS[index as usize] }
}

/// Return a const pointer to the thread with the given ID, or null.
pub fn get_by_id(id: i32) -> *const Thread {
    unsafe {
        for i in 0..MAX_THREADS {
            if THREADS[i].state != ThreadState::Dead && THREADS[i].id == id as u32 {
                return &raw const THREADS[i];
            }
        }
        ptr::null()
    }
}
