// main.rs -- Kernel entry point.
//
// kernel_main() is called from boot.s after the stack is set up.
// It initializes each subsystem in dependency order, enables hardware
// interrupts, and launches the interactive shell.

#![no_std]
#![no_main]
#![allow(static_mut_refs)]
#![allow(unsafe_op_in_unsafe_fn)]

mod io;
mod multiboot;
mod vga;
mod serial;
mod string;
mod gdt;
mod idt;
mod pic;
mod timer;
mod keyboard;
mod pmm;
mod vmm;
mod heap;
mod thread;
mod sync;
mod vfs;
mod initrd;
mod ramfs;
mod devfs;
mod pci;
mod ata;
mod fat16;
mod fat16_vfs;
mod net;
mod arp;
mod ipv4;
mod icmp;
mod udp;
mod tcp;
mod dns;
mod rtl8139;
#[cfg(target_arch = "x86")]
mod rc;
mod elf;
mod syscall;
mod shell;
mod editor;
mod recovery;

use core::arch::asm;
use multiboot::{MultibootInfo, ModEntry, MULTIBOOT_MAGIC, MULTIBOOT_FLAG_MODS, MULTIBOOT_FLAG_MMAP};

unsafe fn print_uint(val: u32) {
    if val == 0 {
        vga::putchar(b'0');
        return;
    }
    let mut buf = [0u8; 12];
    let mut i = 0usize;
    let mut v = val;
    while v > 0 {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i += 1;
    }
    while i > 0 {
        i -= 1;
        vga::putchar(buf[i]);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn kernel_main(magic: u64, mb_info: u64) -> ! {
    unsafe {
        vga::init();
        vga::set_80x50();
        serial::init();
        vga::set_serial_mirror(true);

        if magic as u32 != MULTIBOOT_MAGIC {
            vga::puts(b"ERROR: Not booted via Multiboot!\n");
            loop { asm!("hlt"); }
        }

        let mb = &*(mb_info as *const MultibootInfo);

        pmm::init(mb);

        // Reserve memory used by Multiboot structures
        pmm::reserve_range(
            mb_info,
            mb_info + core::mem::size_of::<MultibootInfo>() as u64,
        );

        if mb.flags & MULTIBOOT_FLAG_MMAP != 0 {
            pmm::reserve_range(mb.mmap_addr as u64, (mb.mmap_addr + mb.mmap_length) as u64);
        }

        if mb.flags & MULTIBOOT_FLAG_MODS != 0 && mb.mods_count > 0 {
            let mod_entry = &*(mb.mods_addr as *const ModEntry);
            pmm::reserve_range(
                mb.mods_addr as u64,
                (mb.mods_addr + mb.mods_count * core::mem::size_of::<ModEntry>() as u32) as u64,
            );
            pmm::reserve_range(mod_entry.mod_start as u64, mod_entry.mod_end as u64);
        }

        vga::puts(b"PMM: ");
        print_uint(pmm::get_free_pages() * 4 / 1024);
        vga::puts(b"MB free (");
        print_uint(pmm::get_free_pages());
        vga::puts(b" pages)\n");

        vmm::init();
        vga::puts(b"VMM: Paging enabled\n");

        // Identity-map multiboot structures and module memory
        // (Multiboot1 addresses are u32, widen to u64 for VMM)
        if mb.flags & MULTIBOOT_FLAG_MODS != 0 && mb.mods_count > 0 {
            let mods_page = (mb.mods_addr & !0xFFF) as u64;
            let mods_end = ((mb.mods_addr
                + mb.mods_count * core::mem::size_of::<ModEntry>() as u32
                + 0xFFF) & !0xFFF) as u64;

            let mut addr = mods_page;
            while addr < mods_end {
                if vmm::get_physical(addr) == 0 {
                    vmm::map_page(addr, addr, vmm::PAGE_PRESENT | vmm::PAGE_WRITE);
                }
                addr += 0x1000;
            }

            let mod_entry = &*(mb.mods_addr as *const ModEntry);
            let start = (mod_entry.mod_start & !0xFFF) as u64;
            let end = ((mod_entry.mod_end + 0xFFF) & !0xFFF) as u64;

            addr = start;
            while addr < end {
                if vmm::get_physical(addr) == 0 {
                    vmm::map_page(addr, addr, vmm::PAGE_PRESENT | vmm::PAGE_WRITE);
                }
                addr += 0x1000;
            }
        }

        heap::init();
        vga::puts(b"Heap: initialized at 0x00500000\n");

        thread::init();
        vga::puts(b"Threads: scheduler initialized\n");

        gdt::init();
        idt::init();
        pic::init();
        timer::init();
        keyboard::init();
        rtl8139::init();
        net::init();
        dns::init();
        tcp::init();

        syscall::init();

        initrd::init(mb);
        vfs::init();

        // Persistent storage: ATA disk + FAT16 (optional)
        if ata::init() == 0 {
            if fat16::init() == 0 {
                fat16_vfs::init();
            }
        }

        asm!("sti"); // enable hardware interrupts

        // Check for "autotest" in kernel command line
        if mb.cmdline != 0 {
            let cmdline = mb.cmdline as *const u8;
            if string::strstr_raw(cmdline, b"autotest\0".as_ptr()) {
                vga::puts(b"Autotest mode\n");
                vga::puts(b"AUTOTEST START\n");

                #[cfg(target_arch = "x86")]
                {
                    vga::puts(b"Compiling hello.rs...\n");
                    if rc::rc_compile(b"hello.rs\0".as_ptr(), b"hello.bin\0".as_ptr()) != 0 {
                        vga::puts(b"FAIL compile hello.rs\n");
                        vga::puts(b"AUTOTEST END\n");
                        loop { asm!("hlt"); }
                    }
                    vga::puts(b"Running hello.bin...\n");
                    if let Some(node) = vfs::finddir_root(b"hello.bin\0".as_ptr()) {
                        let load = rc::emit::CC_LOAD_BASE as *mut u8;
                        let n = vfs::read(node, 0, (*node).size, load);
                        if n > 0 {
                            let entry: extern "C" fn() -> i32 =
                                core::mem::transmute(rc::emit::CC_LOAD_BASE as *const ());
                            let result = entry();
                            vga::puts(b"hello.rs returned ");
                            vga::putchar(b'0' + result as u8);
                            vga::putchar(b'\n');
                        } else {
                            vga::puts(b"FAIL read hello.bin\n");
                        }
                    } else {
                        vga::puts(b"FAIL find hello.bin\n");
                    }

                    vga::puts(b"Compiling test.rs...\n");
                    if rc::rc_compile(b"test.rs\0".as_ptr(), b"test.bin\0".as_ptr()) == 0 {
                        vga::puts(b"Running test.bin...\n");
                        if let Some(node) = vfs::finddir_root(b"test.bin\0".as_ptr()) {
                            let load = rc::emit::CC_LOAD_BASE as *mut u8;
                            let n = vfs::read(node, 0, 65536, load);
                            if n > 0 {
                                let entry: extern "C" fn() -> i32 =
                                    core::mem::transmute(rc::emit::CC_LOAD_BASE as *const ());
                                let result = entry();
                                if result == 0 {
                                    vga::puts(b"AUTOTEST PASS\n");
                                } else {
                                    vga::puts(b"AUTOTEST FAIL\n");
                                }
                            }
                        }
                    } else {
                        vga::puts(b"AUTOTEST COMPILE FAIL\n");
                    }
                }

                #[cfg(target_arch = "x86_64")]
                {
                    vga::puts(b"rc compiler not available on x86_64\n");
                }

                vga::puts(b"AUTOTEST END\n");
                loop { asm!("hlt"); }
            }
        }

        shell::run();
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    unsafe {
        vga::set_color(vga::Color::White, vga::Color::Red);
        vga::puts(b"\n!!! KERNEL PANIC !!!\n");
        if let Some(location) = info.location() {
            // Print file name
            let file = location.file().as_bytes();
            vga::write_bytes(file);
            vga::puts(b":");
            print_uint(location.line());
            vga::putchar(b'\n');
        }
        vga::set_color(vga::Color::LightGrey, vga::Color::Black);
    }
    loop {
        unsafe { asm!("cli"); }
        unsafe { asm!("hlt"); }
    }
}
