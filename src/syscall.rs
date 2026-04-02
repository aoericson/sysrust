// syscall.rs -- Linux-compatible syscall interface.
//
// Supports two entry paths:
// 1. int 0x80 (legacy i386 ABI): nr in rax, args in rbx/rcx/rdx
// 2. syscall instruction (x86_64 ABI): nr in rax, args in rdi/rsi/rdx/r10/r8/r9
//
// The x86_64 syscall numbers follow the Linux convention:
//   0=read, 1=write, 2=open, 3=close, 5=fstat, 8=lseek, 9=mmap,
//   11=munmap, 12=brk, 20=writev, 60=exit, 79=getcwd, 82=rename,
//   83=mkdir, 87=unlink, 217=getdents64, 231=exit_group, 263=unlinkat,
//   318=getrandom, etc.

use core::arch::asm;
use crate::idt::{self, Registers};
use crate::vfs::{self, VfsNode};
use crate::vga;
use crate::vmm;
use crate::pmm;
use crate::string;

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
        8   => sys_lseek(a1, a2, a3),            // lseek(fd, offset, whence)
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
        79  => sys_getcwd(a1, a2),               // getcwd(buf, size)
        82  => sys_rename(a1, a2),               // rename(oldpath, newpath)
        83  => sys_mkdir(a1, a2),                // mkdir(path, mode)
        87  => sys_unlink(a1),                   // unlink(path)
        96  => 0,                                 // gettimeofday stub
        102 => 0,                                 // getuid
        104 => 0,                                 // getgid
        107 => 0,                                 // geteuid
        108 => 0,                                 // getegid
        158 => sys_arch_prctl(a1, a2),             // arch_prctl(code, addr)
        217 => sys_getdents64(a1, a2, a3),       // getdents64(fd, buf, count)
        218 => 0,                                 // set_tid_address stub
        228 => 0,                                 // clock_gettime stub
        231 => { sys_exit(a1); 0 }               // exit_group(code)
        257 => sys_openat(a1, a2, a3),           // openat(dirfd, path, flags)
        263 => sys_unlinkat(a1, a2, a3),         // unlinkat(dirfd, path, flags)
        302 => 0,                                 // prlimit64 stub
        318 => sys_getrandom(a1, a2, a3),        // getrandom(buf, count, flags)
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
            8   => sys_lseek((*regs).rbx, (*regs).rcx, (*regs).rdx),
            19  => sys_lseek((*regs).rbx, (*regs).rcx, (*regs).rdx), // lseek (i386 nr)
            28  => sys_fstat((*regs).rbx, (*regs).rcx),              // fstat (i386 nr)
            39  => sys_mkdir((*regs).rbx, (*regs).rcx),              // mkdir (i386 nr)
            10  => sys_unlink((*regs).rbx),                          // unlink (i386 nr)
            38  => sys_rename((*regs).rbx, (*regs).rcx),             // rename (i386 nr)
            45  => sys_brk((*regs).rbx),
            54  => (-1i64) as u64,
            90  => (-1i64) as u64,
            91  => 0,
            141 => sys_getdents64((*regs).rbx, (*regs).rcx, (*regs).rdx), // getdents64 (approx)
            146 => sys_writev((*regs).rbx, (*regs).rcx, (*regs).rdx),
            183 => sys_getcwd((*regs).rbx, (*regs).rcx),            // getcwd (i386 nr)
            355 => sys_getrandom((*regs).rbx, (*regs).rcx, (*regs).rdx), // getrandom (i386 nr)
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

// ---- lseek: set file offset ---

unsafe fn sys_lseek(fd: u64, offset: u64, whence: u64) -> u64 {
    let result = vfs::vfs_lseek(fd as i32, offset as i64, whence as u32);
    result as u64
}

// ---- fstat: return real file info ---

unsafe fn sys_fstat(fd: u64, statbuf: u64) -> u64 {
    if statbuf == 0 {
        return (-14i64) as u64; // EFAULT
    }

    // Zero the stat buffer (sizeof(struct stat) = 144 on x86_64)
    string::memset(statbuf as *mut u8, 0, 144);

    // Check if fd is valid
    if fd as usize >= vfs::FD_TABLE_SIZE || !vfs::FD_TABLE[fd as usize].in_use {
        return (-9i64) as u64; // EBADF
    }

    let node = vfs::FD_TABLE[fd as usize].node;
    if node.is_null() {
        return (-9i64) as u64; // EBADF
    }

    // Populate st_size at offset 48 (u64) in struct stat
    let size_ptr = (statbuf + 48) as *mut u64;
    *size_ptr = (*node).size as u64;

    // Populate st_mode at offset 24 (u32) with basic type bits
    let mode_ptr = (statbuf + 24) as *mut u32;
    if (*node).flags & vfs::VFS_DIRECTORY != 0 {
        *mode_ptr = 0o040755; // S_IFDIR | rwxr-xr-x
    } else {
        *mode_ptr = 0o100644; // S_IFREG | rw-r--r--
    }

    // Populate st_ino at offset 0 (u64)
    let ino_ptr = statbuf as *mut u64;
    *ino_ptr = (*node).inode as u64;

    // Populate st_blksize at offset 56 (u64) -- advisory block size
    let blksz_ptr = (statbuf + 56) as *mut u64;
    *blksz_ptr = 4096;

    0
}

// ---- getcwd: return current working directory ---

unsafe fn sys_getcwd(buf: u64, size: u64) -> u64 {
    if buf == 0 || size == 0 {
        return (-14i64) as u64; // EFAULT
    }
    // We don't track per-process cwd, always return "/"
    if size < 2 {
        return (-34i64) as u64; // ERANGE
    }
    let p = buf as *mut u8;
    *p = b'/';
    *p.add(1) = 0;
    buf
}

// ---- mkdir: create directory ---

unsafe fn sys_mkdir(path: u64, _mode: u64) -> u64 {
    if path == 0 {
        return (-14i64) as u64; // EFAULT
    }
    let result = vfs::mkdir_p(path as *const u8);
    if result.is_null() {
        return (-12i64) as u64; // ENOMEM
    }
    0
}

// ---- getdents64: list directory contents ---

unsafe fn sys_getdents64(fd: u64, buf: u64, count: u64) -> u64 {
    if buf == 0 || count == 0 {
        return (-14i64) as u64; // EFAULT
    }

    // Validate fd
    if fd as usize >= vfs::FD_TABLE_SIZE || !vfs::FD_TABLE[fd as usize].in_use {
        return (-9i64) as u64; // EBADF
    }

    let node = vfs::FD_TABLE[fd as usize].node;
    if node.is_null() {
        return (-9i64) as u64; // EBADF
    }

    // Must be a directory
    if (*node).flags & vfs::VFS_DIRECTORY == 0 {
        return (-20i64) as u64; // ENOTDIR
    }

    // Use the fd offset to track which entry we're at
    let start_idx = vfs::FD_TABLE[fd as usize].offset;
    let mut pos: u64 = 0; // bytes written into buf
    let mut idx = start_idx;

    loop {
        let child = vfs::readdir(node, idx);
        if child.is_null() {
            break; // no more entries
        }

        // Compute entry size: d_ino(8) + d_off(8) + d_reclen(2) + d_type(1) + name + null
        let name_len = string::strlen((*child).name.as_ptr());
        // reclen must be 8-byte aligned
        let reclen_raw = 8 + 8 + 2 + 1 + name_len + 1; // +1 for null terminator
        let reclen = (reclen_raw + 7) & !7; // align to 8 bytes

        // Check if this entry fits in the remaining buffer
        if pos + reclen as u64 > count {
            break;
        }

        let entry = (buf + pos) as *mut u8;

        // Zero the entry area for padding
        string::memset(entry, 0, reclen);

        // d_ino (offset 0, u64) -- use inode or index
        let d_ino_ptr = entry as *mut u64;
        *d_ino_ptr = if (*child).inode != 0 {
            (*child).inode as u64
        } else {
            (idx + 1) as u64
        };

        // d_off (offset 8, u64) -- offset to next entry
        let d_off_ptr = entry.add(8) as *mut u64;
        *d_off_ptr = (idx + 1) as u64;

        // d_reclen (offset 16, u16)
        let d_reclen_ptr = entry.add(16) as *mut u16;
        *d_reclen_ptr = reclen as u16;

        // d_type (offset 18, u8)
        let d_type_ptr = entry.add(18);
        if (*child).flags & vfs::VFS_DIRECTORY != 0 {
            *d_type_ptr = 4; // DT_DIR
        } else {
            *d_type_ptr = 8; // DT_REG
        }

        // d_name (offset 19, null-terminated)
        string::memcpy(entry.add(19), (*child).name.as_ptr(), name_len);
        *entry.add(19 + name_len) = 0;

        pos += reclen as u64;
        idx += 1;
    }

    // Update the fd offset so the next call continues where we left off
    vfs::FD_TABLE[fd as usize].offset = idx;

    // If we didn't emit any entries but there are no more children, return 0 (EOF)
    // If we didn't emit any entries and there are children but none fit, return -EINVAL
    if pos == 0 && !vfs::readdir(node, start_idx).is_null() {
        return (-22i64) as u64; // EINVAL - buffer too small
    }

    pos
}

// ---- getrandom: fill buffer with pseudo-random bytes ---

unsafe fn sys_getrandom(buf: u64, count: u64, _flags: u64) -> u64 {
    if buf == 0 {
        return (-14i64) as u64; // EFAULT
    }

    let ticks = crate::timer::get_ticks() as u64;
    let ptr = buf as *mut u8;

    // Simple LCG-based PRNG seeded from timer ticks
    let mut state: u64 = ticks.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);

    for i in 0..count as usize {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        *ptr.add(i) = (state >> 33) as u8;
    }

    count
}

// ---- rename: rename/move a file ---

unsafe fn sys_rename(oldpath: u64, newpath: u64) -> u64 {
    if oldpath == 0 || newpath == 0 {
        return (-14i64) as u64; // EFAULT
    }

    let old_path = oldpath as *const u8;
    let new_path = newpath as *const u8;

    // Resolve the old node
    let old_node = vfs::resolve_path(old_path);
    if old_node.is_null() {
        return (-2i64) as u64; // ENOENT
    }

    // Find old parent and old name
    let old_parent = (*old_node).parent;
    if old_parent.is_null() {
        return (-1i64) as u64; // can't rename root
    }

    // Parse the new path to get new parent + new name
    let new_len = string::strlen(new_path);
    if new_len == 0 {
        return (-22i64) as u64; // EINVAL
    }

    // Find last slash in new path
    let mut last_slash: i32 = -1;
    for i in 0..new_len {
        if *new_path.add(i) == b'/' {
            last_slash = i as i32;
        }
    }

    let new_parent: *mut VfsNode;
    let new_name: *const u8;

    if last_slash < 0 {
        // No slash -> new file in root
        new_parent = vfs::VFS_ROOT;
        new_name = new_path;
    } else if last_slash == 0 {
        // Directly under root
        new_parent = vfs::VFS_ROOT;
        new_name = new_path.add(1);
    } else {
        // Build parent path
        let plen = last_slash as usize;
        let mut parent_buf = [0u8; 256];
        if plen >= 255 {
            return (-36i64) as u64; // ENAMETOOLONG
        }
        string::memcpy(parent_buf.as_mut_ptr(), new_path, plen);
        parent_buf[plen] = 0;

        new_parent = vfs::resolve_path(parent_buf.as_ptr());
        if new_parent.is_null() {
            return (-2i64) as u64; // ENOENT
        }
        new_name = new_path.add(last_slash as usize + 1);
    }

    if *new_name == 0 {
        return (-22i64) as u64; // EINVAL
    }

    // Remove old_node from old_parent's children list (without freeing it)
    let old_count = (*old_parent).num_children as usize;
    let mut found_idx: Option<usize> = None;
    for i in 0..old_count {
        if (*old_parent).children[i] == old_node {
            found_idx = Some(i);
            break;
        }
    }

    match found_idx {
        None => return (-2i64) as u64, // ENOENT
        Some(idx) => {
            let last = old_count - 1;
            for i in idx..last {
                (*old_parent).children[i] = (*old_parent).children[i + 1];
            }
            (*old_parent).children[last] = core::ptr::null_mut();
            (*old_parent).num_children -= 1;
        }
    }

    // Rename the node
    let name_len = string::strlen(new_name);
    let copy_len = if name_len > 63 { 63 } else { name_len };
    string::memset((*old_node).name.as_mut_ptr(), 0, 64);
    string::memcpy((*old_node).name.as_mut_ptr(), new_name, copy_len);
    (*old_node).name[copy_len] = 0;

    // Add to new parent
    if vfs::add_child(new_parent, old_node) < 0 {
        // Try to re-add to old parent as fallback
        let _ = vfs::add_child(old_parent, old_node);
        return (-28i64) as u64; // ENOSPC
    }

    0
}

// ---- unlink: delete a file ---

unsafe fn sys_unlink(path: u64) -> u64 {
    if path == 0 {
        return (-14i64) as u64; // EFAULT
    }
    let result = vfs::unlink_path(path as *const u8);
    if result < 0 {
        return (-2i64) as u64; // ENOENT
    }
    0
}

// ---- unlinkat: delete a file relative to dirfd ---

unsafe fn sys_unlinkat(_dirfd: u64, path: u64, _flags: u64) -> u64 {
    // Ignore dirfd, treat path as absolute or relative to root
    sys_unlink(path)
}

unsafe fn sys_brk(addr: u64) -> u64 {
    if addr == 0 {
        return CURRENT_BRK;
    }
    if addr < CURRENT_BRK {
        return CURRENT_BRK;
    }
    // Pages within first 1GB already mapped by boot
    if addr >= 0x1_0000_0000 {
        let mut page = CURRENT_BRK & !0xFFF;
        let end_page = (addr + 0xFFF) & !0xFFF;
        while page < end_page {
            if page >= 0x1_0000_0000 {
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

/// mmap -- allocate anonymous memory (MAP_ANONYMOUS) or stub for file mapping.
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

/// arch_prctl -- set/get architecture-specific thread state.
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
