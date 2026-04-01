// syscall.rs -- Linux-compatible syscall interface via int 0x80.
//
// Programs call int 0x80 with rax=syscall number, args in rbx/rcx/rdx/rsi/rdi.
// Return value placed in rax. This uses the same register convention as Linux
// i386 (eax=nr, ebx/ecx/edx=args) widened to 64-bit registers, so existing
// test programs work without modification.

use crate::idt::{self, Registers};
use crate::vfs;
use crate::vga;
use crate::vmm;
use crate::pmm;

// Current program break (end of allocated program memory).
// Set by the ELF loader before execution, extended by sys_brk.
static mut CURRENT_BRK: u64 = 0;

pub unsafe fn init() {
    crate::idt::install_isr128();
    idt::register_handler(128, syscall_handler);
}

/// Set the initial program break (called by ELF loader).
pub unsafe fn set_brk(brk: u64) {
    CURRENT_BRK = brk;
}

fn syscall_handler(regs: *mut Registers) {
    unsafe {
        let nr = (*regs).rax;
        let ret: u64 = match nr {
            1   => sys_exit((*regs).rbx),
            3   => sys_read((*regs).rbx, (*regs).rcx, (*regs).rdx),
            4   => sys_write((*regs).rbx, (*regs).rcx, (*regs).rdx),
            5   => sys_open((*regs).rbx),
            6   => sys_close((*regs).rbx),
            45  => sys_brk((*regs).rbx),
            54  => (-1i64) as u64,  // ioctl stub
            90  => (-1i64) as u64,  // mmap stub
            91  => 0,               // munmap stub
            146 => sys_writev((*regs).rbx, (*regs).rcx, (*regs).rdx),
            174 => 0,               // sigaction stub (return success)
            175 => 0,               // sigprocmask stub
            192 => (-1i64) as u64,  // mmap2 stub
            195 => (-1i64) as u64,  // stat64 stub
            197 => (-1i64) as u64,  // fstat64 stub
            220 => 0,               // getdents64 stub
            243 => 0,               // set_thread_area stub
            _   => {
                // Unknown syscall -- return -ENOSYS
                (-38i64) as u64
            }
        };
        (*regs).rax = ret;
    }
}

unsafe fn sys_exit(code: u64) -> u64 {
    // Signal recovery.rs that exit was intentional (not a crash)
    crate::recovery::set_child_result(code as i32);
    crate::thread::exit();
    0
}

unsafe fn sys_read(fd: u64, buf: u64, len: u64) -> u64 {
    if fd == 0 {
        // stdin: read from keyboard
        if len == 0 { return 0; }
        let c = crate::keyboard::getchar();
        *(buf as *mut u8) = c;
        return 1;
    }
    vfs::vfs_fd_read(fd as i32, buf as *mut u8, len as u32) as u64
}

unsafe fn sys_write(fd: u64, buf: u64, len: u64) -> u64 {
    // fd 1 = stdout, fd 2 = stderr -> VGA output
    if fd == 1 || fd == 2 {
        for i in 0..len {
            vga::putchar(*(buf as *const u8).add(i as usize));
        }
        return len;
    }
    vfs::vfs_fd_write(fd as i32, buf as *const u8, len as u32) as u64
}

unsafe fn sys_open(path_ptr: u64) -> u64 {
    vfs::vfs_open(path_ptr as *const u8) as u64
}

unsafe fn sys_close(fd: u64) -> u64 {
    vfs::vfs_close(fd as i32);
    0
}

unsafe fn sys_brk(addr: u64) -> u64 {
    if addr == 0 {
        return CURRENT_BRK;
    }
    if addr < CURRENT_BRK {
        // Shrinking not supported, just return current
        return CURRENT_BRK;
    }
    // Map pages between current break and requested address.
    // Pages within the first 1GB are already identity-mapped by the boot
    // assembly using 2MB huge pages, so we only need to allocate for
    // addresses above 1GB.
    if addr >= 0x4000_0000 {
        let mut page = CURRENT_BRK & !0xFFF;
        let end_page = (addr + 0xFFF) & !0xFFF;
        while page < end_page {
            if page >= 0x4000_0000 {
                let phys = pmm::alloc_page();
                if phys == 0 {
                    return CURRENT_BRK; // OOM
                }
                vmm::map_page(page, phys, vmm::PAGE_PRESENT | vmm::PAGE_WRITE);
            }
            page += 0x1000;
        }
    }
    CURRENT_BRK = addr;
    addr
}

/// writev(fd, iov, iovcnt) -- gather write. Used by Rust's write! macro.
unsafe fn sys_writev(fd: u64, iov_ptr: u64, iovcnt: u64) -> u64 {
    // struct iovec { void *iov_base; size_t iov_len; } -- 16 bytes on x86_64
    let mut total: u64 = 0;
    for i in 0..iovcnt {
        let entry = (iov_ptr + i * 16) as *const u64;
        let base = *entry;
        let len = *entry.add(1);
        total += sys_write(fd, base, len);
    }
    total
}
