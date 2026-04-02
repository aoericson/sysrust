// syscall.rs -- Linux-compatible syscall interface.
//
// Supports two entry paths:
// 1. int 0x80 (legacy i386 ABI): nr in rax, args in rbx/rcx/rdx
// 2. syscall instruction (x86_64 ABI): nr in rax, args in rdi/rsi/rdx/r10/r8/r9
//
// The x86_64 syscall numbers follow the Linux convention:
//   0=read, 1=write, 2=open, 3=close, 9=mmap, 11=munmap, 12=brk,
//   20=writev, 60=exit, 231=exit_group, etc.

use core::arch::asm;
use crate::idt::{self, Registers};
use crate::vfs;
use crate::vga;
use crate::vmm;
use crate::pmm;

// Current program break
static mut CURRENT_BRK: u64 = 0;

// Next anonymous mmap address (grows upward from 0x7F000000)
static mut MMAP_NEXT: u64 = 0x7F00_0000;

// ---- MSR addresses for syscall instruction ----
const MSR_STAR: u32 = 0xC000_0081;
const MSR_LSTAR: u32 = 0xC000_0082;
const MSR_SFMASK: u32 = 0xC000_0084;
const MSR_EFER: u32 = 0xC000_0080;

unsafe fn wrmsr(msr: u32, value: u64) {
    let low = value as u32;
    let high = (value >> 32) as u32;
    asm!("wrmsr", in("ecx") msr, in("eax") low, in("edx") high);
}

unsafe fn rdmsr(msr: u32) -> u64 {
    let low: u32;
    let high: u32;
    asm!("rdmsr", in("ecx") msr, out("eax") low, out("edx") high);
    (high as u64) << 32 | low as u64
}

pub unsafe fn init() {
    // Install int 0x80 handler (legacy path for sysrust-rt programs)
    crate::idt::install_isr128();
    idt::register_handler(128, int80_handler);

    // ==== Linux x86_64 syscall instruction support ====
    // This enables running statically-linked Linux ELF binaries inside the OS.
    // Tagged: LINUX_COMPAT — can be removed if Linux compatibility is not needed.
    unsafe extern "C" { fn syscall_entry(); }

    // Enable syscall/sysret via EFER.SCE (bit 0)
    let efer = rdmsr(MSR_EFER);
    wrmsr(MSR_EFER, efer | 1);

    // STAR: bits 47:32 = kernel CS (0x08), bits 63:48 = user CS base (unused, ring 0)
    wrmsr(MSR_STAR, (0x08u64 << 32) | (0x08u64 << 48));

    // LSTAR: address of the assembly syscall entry point
    wrmsr(MSR_LSTAR, syscall_entry as u64);

    // SFMASK: clear IF (bit 9) on syscall entry to disable interrupts
    wrmsr(MSR_SFMASK, 0x200);
}

pub unsafe fn set_brk(brk: u64) {
    CURRENT_BRK = brk;
}

// ---- syscall instruction entry point ----
// On syscall: RCX = return RIP, R11 = saved RFLAGS
// Args: rax=nr, rdi=arg1, rsi=arg2, rdx=arg3, r10=arg4, r8=arg5, r9=arg6

// syscall_entry is now in boot/isr.s (pure assembly, no Rust prologue)
// It calls syscall_dispatch_x64 below.

// ---- x86_64 Linux syscall dispatch ----

#[unsafe(no_mangle)]
unsafe extern "C" fn syscall_dispatch_x64(
    nr: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64,
) -> u64 {
    match nr {
        0   => sys_read(a1, a2, a3),             // read(fd, buf, count)
        1   => sys_write(a1, a2, a3),            // write(fd, buf, count)
        2   => sys_open(a1, a2),                 // open(path, flags)
        3   => sys_close(a1),                    // close(fd)
        5   => sys_fstat(a1, a2),                // fstat(fd, statbuf)
        9   => sys_mmap(a1, a2, a3, a4, a5),     // mmap(addr, len, prot, flags, fd)
        10  => 0,                                 // mprotect stub
        11  => sys_munmap(a1, a2),               // munmap(addr, len)
        12  => sys_brk(a1),                      // brk(addr)
        13  => 0,                                 // rt_sigaction stub
        14  => 0,                                 // rt_sigprocmask stub
        16  => sys_ioctl(a1, a2),                // ioctl(fd, cmd)
        20  => sys_writev(a1, a2, a3),           // writev(fd, iov, iovcnt)
        21  => 0,                                 // access stub (return success)
        39  => 1,                                 // getpid: return 1
        60  => { sys_exit(a1); 0 }               // exit(code)
        63  => 0,                                 // uname stub
        72  => 0,                                 // fcntl stub
        79  => 0,                                 // getcwd stub
        96  => 0,                                 // gettimeofday stub
        102 => 0,                                 // getuid
        104 => 0,                                 // getgid
        107 => 0,                                 // geteuid
        108 => 0,                                 // getegid
        158 => sys_arch_prctl(a1, a2),             // arch_prctl(code, addr)
        218 => 0,                                 // set_tid_address stub
        228 => 0,                                 // clock_gettime stub
        231 => { sys_exit(a1); 0 }               // exit_group(code)
        257 => sys_openat(a1, a2, a3),           // openat(dirfd, path, flags)
        302 => 0,                                 // prlimit64 stub
        334 => 0,                                 // rseq stub
        _   => {
            // Unknown syscall -- return -ENOSYS
            (-38i64) as u64
        }
    }
}

// ---- int 0x80 handler (legacy i386 ABI, used by sysrust-rt programs) ----

fn int80_handler(regs: *mut Registers) {
    unsafe {
        let nr = (*regs).rax;
        // Legacy i386 ABI: args in rbx, rcx, rdx
        let ret: u64 = match nr {
            1   => { sys_exit((*regs).rbx); 0 }
            3   => sys_read((*regs).rbx, (*regs).rcx, (*regs).rdx),
            4   => sys_write((*regs).rbx, (*regs).rcx, (*regs).rdx),
            5   => sys_open((*regs).rbx, 0),
            6   => sys_close((*regs).rbx),
            45  => sys_brk((*regs).rbx),
            54  => (-1i64) as u64,
            90  => (-1i64) as u64,
            91  => 0,
            146 => sys_writev((*regs).rbx, (*regs).rcx, (*regs).rdx),
            _   => (-38i64) as u64,
        };
        (*regs).rax = ret;
    }
}

// ---- syscall implementations ----

unsafe fn sys_exit(code: u64) -> u64 {
    crate::recovery::set_child_result(code as i32);
    crate::thread::exit();
    0
}

unsafe fn sys_read(fd: u64, buf: u64, len: u64) -> u64 {
    if fd == 0 {
        if len == 0 { return 0; }
        let c = crate::keyboard::getchar();
        *(buf as *mut u8) = c;
        return 1;
    }
    vfs::vfs_fd_read(fd as i32, buf as *mut u8, len as u32) as u64
}

unsafe fn sys_write(fd: u64, buf: u64, len: u64) -> u64 {
    if fd == 1 || fd == 2 {
        for i in 0..len {
            vga::putchar(*(buf as *const u8).add(i as usize));
        }
        return len;
    }
    vfs::vfs_fd_write(fd as i32, buf as *const u8, len as u32) as u64
}

unsafe fn sys_open(path_ptr: u64, _flags: u64) -> u64 {
    vfs::vfs_open(path_ptr as *const u8) as u64
}

unsafe fn sys_openat(_dirfd: u64, path_ptr: u64, _flags: u64) -> u64 {
    // Ignore dirfd, just open by name
    vfs::vfs_open(path_ptr as *const u8) as u64
}

unsafe fn sys_close(fd: u64) -> u64 {
    vfs::vfs_close(fd as i32);
    0
}

unsafe fn sys_ioctl(_fd: u64, _cmd: u64) -> u64 {
    // Stub: return -ENOTTY for terminal ioctls
    (-25i64) as u64
}

unsafe fn sys_fstat(_fd: u64, _statbuf: u64) -> u64 {
    // Stub: zero the stat buffer and return success
    if _statbuf != 0 {
        crate::string::memset(_statbuf as *mut u8, 0, 144); // sizeof(struct stat) on x86_64
    }
    0
}

unsafe fn sys_brk(addr: u64) -> u64 {
    if addr == 0 {
        return CURRENT_BRK;
    }
    if addr < CURRENT_BRK {
        return CURRENT_BRK;
    }
    // Pages within first 1GB already mapped by boot
    if addr >= 0x4000_0000 {
        let mut page = CURRENT_BRK & !0xFFF;
        let end_page = (addr + 0xFFF) & !0xFFF;
        while page < end_page {
            if page >= 0x4000_0000 {
                let phys = pmm::alloc_page();
                if phys == 0 { return CURRENT_BRK; }
                vmm::map_page(page, phys, vmm::PAGE_PRESENT | vmm::PAGE_WRITE);
            }
            page += 0x1000;
        }
    }
    CURRENT_BRK = addr;
    addr
}

/// mmap — allocate anonymous memory (MAP_ANONYMOUS) or stub for file mapping.
unsafe fn sys_mmap(addr: u64, len: u64, _prot: u64, flags: u64, _fd: u64) -> u64 {
    if len == 0 {
        return (-22i64) as u64; // EINVAL
    }

    let pages = (len + 0xFFF) / 0x1000;

    // Pick an address
    let vaddr = if addr != 0 && (flags & 0x10) != 0 {
        // MAP_FIXED: use requested address
        addr & !0xFFF
    } else {
        // Auto-assign from MMAP_NEXT
        let v = (MMAP_NEXT + 0xFFF) & !0xFFF;
        MMAP_NEXT = v + pages * 0x1000;
        v
    };

    // Allocate physical pages and map
    for i in 0..pages {
        let page = vaddr + i * 0x1000;
        let phys = pmm::alloc_page();
        if phys == 0 {
            return (-12i64) as u64; // ENOMEM
        }
        vmm::map_page(page, phys, vmm::PAGE_PRESENT | vmm::PAGE_WRITE | vmm::PAGE_USER);
    }

    // Zero the memory (MAP_ANONYMOUS)
    crate::string::memset(vaddr as *mut u8, 0, (pages * 0x1000) as usize);

    vaddr
}

/// arch_prctl — set/get architecture-specific thread state.
/// ARCH_SET_FS (0x1002) sets the FS segment base for TLS.
unsafe fn sys_arch_prctl(code: u64, addr: u64) -> u64 {
    const ARCH_SET_FS: u64 = 0x1002;
    const ARCH_SET_GS: u64 = 0x1001;

    match code {
        ARCH_SET_FS => {
            // Write FS base via MSR 0xC0000100
            wrmsr(0xC000_0100, addr);
            0
        }
        ARCH_SET_GS => {
            // Write GS base via MSR 0xC0000101
            wrmsr(0xC000_0101, addr);
            0
        }
        _ => (-22i64) as u64, // EINVAL
    }
}

unsafe fn sys_munmap(_addr: u64, _len: u64) -> u64 {
    // Stub: don't actually unmap (leak pages)
    0
}

unsafe fn sys_writev(fd: u64, iov_ptr: u64, iovcnt: u64) -> u64 {
    let mut total: u64 = 0;
    for i in 0..iovcnt {
        let entry = (iov_ptr + i * 16) as *const u64; // iovec is 16 bytes on x86_64
        let base = *entry;
        let len = *entry.add(1);
        total += sys_write(fd, base, len);
    }
    total
}
