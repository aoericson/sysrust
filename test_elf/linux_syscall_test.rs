#![no_std]
#![no_main]
#![allow(unsafe_op_in_unsafe_fn)]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }

/// Linux x86_64 syscall: write(fd, buf, len)
unsafe fn sys_write(fd: u64, buf: *const u8, len: u64) -> i64 {
    let ret: i64;
    core::arch::asm!(
        "syscall",
        in("rax") 1u64,    // __NR_write
        in("rdi") fd,
        in("rsi") buf as u64,
        in("rdx") len,
        lateout("rax") ret,
        out("rcx") _,
        out("r11") _,
    );
    ret
}

/// Linux x86_64 syscall: exit_group(code)
unsafe fn sys_exit(code: u64) -> ! {
    core::arch::asm!(
        "syscall",
        in("rax") 231u64,  // __NR_exit_group
        in("rdi") code,
        options(noreturn),
    );
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    unsafe {
        let msg = b"Hello via Linux syscall instruction!\n";
        sys_write(1, msg.as_ptr(), msg.len() as u64);
        sys_exit(0);
    }
}
