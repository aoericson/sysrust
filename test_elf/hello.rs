#![no_std]
#![no_main]
#![allow(unsafe_op_in_unsafe_fn)]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

unsafe fn syscall1(nr: u32, arg1: u32) -> u32 {
    let ret: u32;
    unsafe {
        core::arch::asm!(
            "int 0x80",
            in("eax") nr,
            in("ebx") arg1,
            lateout("eax") ret,
            out("ecx") _,
            out("edx") _,
        );
    }
    ret
}

unsafe fn syscall3(nr: u32, arg1: u32, arg2: u32, arg3: u32) -> u32 {
    let ret: u32;
    unsafe {
        core::arch::asm!(
            "int 0x80",
            in("eax") nr,
            in("ebx") arg1,
            in("ecx") arg2,
            in("edx") arg3,
            lateout("eax") ret,
        );
    }
    ret
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    unsafe {
        let msg = b"Hello from ELF!\n";
        syscall3(4, 1, msg.as_ptr() as u32, msg.len() as u32);
        syscall1(1, 0);
    }
    loop {}
}
