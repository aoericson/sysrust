//! Global allocator backed by sys_brk.
//!
//! Simple bump allocator that extends the program break on every allocation.
//! Deallocation is a no-op (memory is not reclaimed until the program exits).
//! This is sufficient for running compilers and other short-lived tools.

use core::alloc::{GlobalAlloc, Layout};
use crate::sys;

pub struct BrkAllocator;

static mut HEAP_START: u64 = 0;
static mut HEAP_END: u64 = 0;
static mut HEAP_POS: u64 = 0;

unsafe impl GlobalAlloc for BrkAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // Initialize heap on first call
        if HEAP_START == 0 {
            let current = sys::brk(0);
            // Start heap 4KB above current break
            HEAP_START = (current + 0xFFF) & !0xFFF;
            HEAP_POS = HEAP_START;
            HEAP_END = HEAP_START;
        }

        // Align up
        let align = layout.align() as u64;
        let pos = (HEAP_POS + align - 1) & !(align - 1);
        let new_pos = pos + layout.size() as u64;

        // Extend if needed
        if new_pos > HEAP_END {
            let needed = (new_pos - HEAP_END + 0xFFF) & !0xFFF; // round up to page
            let new_end = HEAP_END + needed;
            let result = sys::brk(new_end);
            if result < new_end {
                return core::ptr::null_mut(); // OOM
            }
            HEAP_END = new_end;
        }

        HEAP_POS = new_pos;
        pos as *mut u8
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // Bump allocator — dealloc is a no-op
    }
}
