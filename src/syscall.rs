// syscall.rs -- Linux-compatible syscall interface via int 0x80.
//
// Programs call int 0x80 with eax=syscall number, args in ebx/ecx/edx/esi/edi.
// Return value placed in eax. This uses the same register convention as Linux
// i386 so cross-compiled programs work without modification.

use crate::idt::{self, Registers};
use crate::vfs;
use crate::vga;
use crate::vmm;
use crate::pmm;

// Current program break (end of allocated program memory).
// Set by the ELF loader before execution, extended by sys_brk.
static mut CURRENT_BRK: u32 = 0;

pub unsafe fn init() {
    crate::idt::install_isr128();
    idt::register_handler(128, syscall_handler);
}

/// Set the initial program break (called by ELF loader).
pub unsafe fn set_brk(brk: u32) {
    CURRENT_BRK = brk;
}

fn syscall_handler(regs: *mut Registers) {
    unsafe {
        let nr = (*regs).eax;
        let ret: u32 = match nr {
            1   => sys_exit((*regs).ebx),
            3   => sys_read((*regs).ebx, (*regs).ecx, (*regs).edx),
            4   => sys_write((*regs).ebx, (*regs).ecx, (*regs).edx),
            5   => sys_open((*regs).ebx),
            6   => sys_close((*regs).ebx),
            45  => sys_brk((*regs).ebx),
            54  => (-1i32) as u32,  // ioctl stub
            90  => (-1i32) as u32,  // mmap stub
            91  => 0,               // munmap stub
            146 => sys_writev((*regs).ebx, (*regs).ecx, (*regs).edx),
            174 => 0,               // sigaction stub (return success)
            175 => 0,               // sigprocmask stub
            192 => (-1i32) as u32,  // mmap2 stub
            195 => (-1i32) as u32,  // stat64 stub
            197 => (-1i32) as u32,  // fstat64 stub
            220 => 0,               // getdents64 stub
            243 => 0,               // set_thread_area stub
            _   => {
                // Unknown syscall — return -ENOSYS
                (-38i32) as u32
            }
        };
        (*regs).eax = ret;
    }
}

unsafe fn sys_exit(code: u32) -> u32 {
    // Signal recovery.rs that exit was intentional (not a crash)
    crate::recovery::set_child_result(code as i32);
    crate::thread::exit();
    0
}

unsafe fn sys_read(fd: u32, buf: u32, len: u32) -> u32 {
    if fd == 0 {
        // stdin: read from keyboard
        if len == 0 { return 0; }
        let c = crate::keyboard::getchar();
        *(buf as *mut u8) = c;
        return 1;
    }
    vfs::vfs_fd_read(fd as i32, buf as *mut u8, len) as u32
}

unsafe fn sys_write(fd: u32, buf: u32, len: u32) -> u32 {
    // fd 1 = stdout, fd 2 = stderr → VGA output
    if fd == 1 || fd == 2 {
        for i in 0..len {
            vga::putchar(*(buf as *const u8).add(i as usize));
        }
        return len;
    }
    vfs::vfs_fd_write(fd as i32, buf as *const u8, len) as u32
}

unsafe fn sys_open(path_ptr: u32) -> u32 {
    vfs::vfs_open(path_ptr as *const u8) as u32
}

unsafe fn sys_close(fd: u32) -> u32 {
    vfs::vfs_close(fd as i32);
    0
}

unsafe fn sys_brk(addr: u32) -> u32 {
    if addr == 0 {
        return CURRENT_BRK;
    }
    if addr < CURRENT_BRK {
        // Shrinking not supported, just return current
        return CURRENT_BRK;
    }
    // Map pages between current break and requested address
    let mut page = CURRENT_BRK & !0xFFF;
    let end_page = (addr + 0xFFF) & !0xFFF;
    while page < end_page {
        if vmm::get_physical(page) == 0 {
            let phys = pmm::alloc_page();
            if phys == 0 {
                return CURRENT_BRK; // OOM
            }
            vmm::map_page(page, phys, vmm::PAGE_PRESENT | vmm::PAGE_WRITE);
        }
        page += 0x1000;
    }
    CURRENT_BRK = addr;
    addr
}

/// writev(fd, iov, iovcnt) — gather write. Used by Rust's write! macro.
unsafe fn sys_writev(fd: u32, iov_ptr: u32, iovcnt: u32) -> u32 {
    // struct iovec { void *iov_base; size_t iov_len; }
    let mut total: u32 = 0;
    for i in 0..iovcnt {
        let entry = (iov_ptr + i * 8) as *const u32;
        let base = *entry;
        let len = *entry.add(1);
        total += sys_write(fd, base, len);
    }
    total
}
