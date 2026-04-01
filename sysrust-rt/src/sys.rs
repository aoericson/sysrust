//! Raw syscall wrappers for sysrust OS.
//!
//! Uses int 0x80 with args in rbx/rcx/rdx (Linux i386 convention widened).
//! Since LLVM reserves rbx, we save/restore it manually around the syscall.

use core::arch::asm;

#[inline]
unsafe fn syscall1(nr: u64, arg1: u64) -> u64 {
    let ret: u64;
    asm!(
        "push rbx",
        "mov rbx, {a1}",
        "int 0x80",
        "pop rbx",
        a1 = in(reg) arg1,
        in("rax") nr,
        lateout("rax") ret,
        out("rcx") _,
        out("rdx") _,
    );
    ret
}

#[inline]
unsafe fn syscall2(nr: u64, arg1: u64, arg2: u64) -> u64 {
    let ret: u64;
    asm!(
        "push rbx",
        "mov rbx, {a1}",
        "int 0x80",
        "pop rbx",
        a1 = in(reg) arg1,
        in("rax") nr,
        in("rcx") arg2,
        lateout("rax") ret,
        out("rdx") _,
    );
    ret
}

#[inline]
unsafe fn syscall3(nr: u64, arg1: u64, arg2: u64, arg3: u64) -> u64 {
    let ret: u64;
    asm!(
        "push rbx",
        "mov rbx, {a1}",
        "int 0x80",
        "pop rbx",
        a1 = in(reg) arg1,
        in("rax") nr,
        in("rcx") arg2,
        in("rdx") arg3,
        lateout("rax") ret,
    );
    ret
}

/// Write bytes to a file descriptor. Returns bytes written.
pub fn write(fd: u32, buf: &[u8]) -> i64 {
    unsafe { syscall3(4, fd as u64, buf.as_ptr() as u64, buf.len() as u64) as i64 }
}

/// Read bytes from a file descriptor. Returns bytes read.
pub fn read(fd: u32, buf: &mut [u8]) -> i64 {
    unsafe { syscall3(3, fd as u64, buf.as_mut_ptr() as u64, buf.len() as u64) as i64 }
}

/// Open a file by path. Returns file descriptor.
pub fn open(path: &[u8]) -> i32 {
    unsafe { syscall1(5, path.as_ptr() as u64) as i32 }
}

/// Close a file descriptor.
pub fn close(fd: u32) {
    unsafe { syscall1(6, fd as u64); }
}

/// Exit the process with a status code. Does not return.
pub fn exit(code: u32) -> ! {
    unsafe {
        syscall1(1, code as u64);
        loop { asm!("hlt"); }
    }
}

/// Adjust the program break (heap end). Returns new break address.
pub fn brk(addr: u64) -> u64 {
    unsafe { syscall1(45, addr) }
}

/// Print a string to stdout.
pub fn print(s: &str) {
    write(1, s.as_bytes());
}

/// Print a string to stdout followed by a newline.
pub fn println(s: &str) {
    write(1, s.as_bytes());
    write(1, b"\n");
}
