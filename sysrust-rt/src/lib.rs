//! sysrust-rt — Runtime for programs running inside sysrust OS.
//!
//! Provides:
//! - Syscall wrappers (write, read, open, close, exit, brk)
//! - A global allocator backed by sys_brk
//! - Entry point (_start) that calls main() and exits
//! - Panic handler
//!
//! Programs link against this crate and use standard Rust with `alloc`.

#![no_std]
#![allow(unsafe_op_in_unsafe_fn)]
#![feature(lang_items)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod sys;

#[cfg(feature = "alloc")]
mod allocator;

#[cfg(feature = "alloc")]
#[global_allocator]
static ALLOC: allocator::BrkAllocator = allocator::BrkAllocator;

/// Panic handler — prints message and exits with code 101.
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let _ = sys::write(2, b"PANIC: ");
    if let Some(msg) = info.message().as_str() {
        let _ = sys::write(2, msg.as_bytes());
    }
    let _ = sys::write(2, b"\n");
    sys::exit(101);
}

/// Entry point called by the OS. Calls user's main() and exits.
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    unsafe extern "Rust" {
        fn main() -> i32;
    }
    let code = unsafe { main() };
    sys::exit(code as u32);
}
