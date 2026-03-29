// recovery.rs -- Crash-safe execution for compiled programs.
//
// run_protected() spawns the compiled program in a fresh kernel thread.
// The parent (shell) thread busy-waits by yielding the CPU until either:
//   - child_done is set to 1 by the child wrapper (normal exit), or
//   - the child thread disappears from the thread table (killed by a
//     CPU exception via thread::exit() in the exception handler).
//
// Global state is protected by the single-threaded nature of the wait
// loop: the shell thread does not call run_protected() recursively.

use crate::thread;
use core::ptr;

/// Shared state between the shell thread and the child wrapper thread.
/// Only one compiled program runs at a time (the shell is synchronous),
/// so plain globals are sufficient -- no locking needed.
static mut CHILD_DONE: i32 = 0;
static mut CHILD_RESULT: i32 = 0;
static mut CHILD_ENTRY: *const () = ptr::null();

/// child_wrapper -- entry point for the child thread.
///
/// Casts CHILD_ENTRY to a function returning i32, calls it, stores the
/// return value, signals completion, then exits the thread cleanly.
extern "C" fn child_wrapper() {
    unsafe {
        let entry: extern "C" fn() -> i32 = core::mem::transmute(CHILD_ENTRY);
        let result = entry();
        core::ptr::write_volatile(&raw mut CHILD_RESULT, result);
        core::ptr::write_volatile(&raw mut CHILD_DONE, 1);
        thread::exit();
    }
}

/// Set the child result and done flag from outside (used by sys_exit).
pub unsafe fn set_child_result(result: i32) {
    core::ptr::write_volatile(&raw mut CHILD_RESULT, result);
    core::ptr::write_volatile(&raw mut CHILD_DONE, 1);
}

/// Run entry_point in a protected thread.
///
/// The calling thread yields the CPU in a loop until the child thread
/// either completes normally (CHILD_DONE == 1) or is killed by the
/// exception handler (thread not alive anymore).
///
/// Returns the child's return value, or -99 on fault/creation failure.
pub fn run_protected(entry: extern "C" fn()) -> i32 {
    unsafe {
        CHILD_ENTRY = entry as *const ();
        core::ptr::write_volatile(&raw mut CHILD_RESULT, -99);
        core::ptr::write_volatile(&raw mut CHILD_DONE, 0);

        let tid = thread::create(child_wrapper);
        if tid < 0 {
            return -99;
        }

        // Wait for child to complete or crash
        loop {
            if core::ptr::read_volatile(&raw const CHILD_DONE) != 0 {
                break;
            }

            // Check if the thread has been killed (e.g., by an exception)
            if !thread::is_alive(tid) {
                break;
            }

            // Yield to let the child (or timer) run
            thread::yield_thread();
        }

        core::ptr::read_volatile(&raw const CHILD_RESULT)
    }
}
