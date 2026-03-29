// vmm.rs -- Virtual memory manager (x86 paging).
//
// Two-level page translation:
//   Page Directory (1024 entries) -> Page Tables (1024 entries each) -> 4KB pages
//
// On init we identity-map the first 16MB so that phys == virt for the
// kernel, VGA buffer, DMA regions, PMM bitmap, and the page tables
// themselves.  This guarantees the code that enables paging is still
// mapped the instant the PG bit is set in CR0.

use core::arch::asm;
use crate::pmm;

pub const VMM_PAGE_SIZE: u32 = 4096;

// Page flags
pub const PAGE_PRESENT: u32 = 0x01;
pub const PAGE_WRITE: u32 = 0x02;
pub const PAGE_USER: u32 = 0x04;

// ---- private state ---------------------------------------------------------

static mut PAGE_DIRECTORY: *mut u32 = core::ptr::null_mut();
static mut PD_PHYS: u32 = 0;

// ---- helpers ---------------------------------------------------------------

#[inline]
unsafe fn invlpg(virt: u32) {
    asm!("invlpg [{}]", in(reg) virt, options(nostack, preserves_flags));
}

/// Zero `count` bytes at `dest`. Used in place of memset for page-sized zeroing.
unsafe fn zero_page(dest: *mut u8, count: usize) {
    let mut i = 0;
    while i < count {
        *dest.add(i) = 0;
        i += 1;
    }
}

// ---- public API ------------------------------------------------------------

pub unsafe fn init() {
    // Allocate and zero the page directory
    PD_PHYS = pmm::alloc_page();
    PAGE_DIRECTORY = PD_PHYS as *mut u32; // phys == virt in flat mode
    zero_page(PAGE_DIRECTORY as *mut u8, VMM_PAGE_SIZE as usize);

    // Identity-map the first 16MB (4 page-directory entries, 4 page tables).
    // Each page table covers 4MB (1024 entries * 4KB).
    for i in 0u32..4 {
        let pt_phys = pmm::alloc_page();
        let pt = pt_phys as *mut u32;

        for j in 0u32..1024 {
            let phys_addr = (i * 1024 + j) * VMM_PAGE_SIZE;
            *pt.add(j as usize) = phys_addr | PAGE_PRESENT | PAGE_WRITE;
        }

        *PAGE_DIRECTORY.add(i as usize) = pt_phys | PAGE_PRESENT | PAGE_WRITE;
    }

    // Load CR3 and enable paging
    asm!(
        "mov cr3, {0}",
        "mov {1}, cr0",
        "or  {1}, 0x80000000",
        "mov cr0, {1}",
        in(reg) PD_PHYS,
        out(reg) _,
    );
}

pub unsafe fn map_page(virt: u32, phys: u32, flags: u32) {
    let pd_idx = (virt >> 22) as usize;
    let pt_idx = ((virt >> 12) & 0x3FF) as usize;

    // Allocate a page table if the directory entry is not present
    if (*PAGE_DIRECTORY.add(pd_idx)) & PAGE_PRESENT == 0 {
        let pt_phys = pmm::alloc_page();
        // Guard against physical address outside identity-mapped region
        if pt_phys >= 0x1000000 {
            crate::vga::puts(b"vmm: PANIC: page table phys addr outside identity map\n");
            return;
        }
        zero_page(pt_phys as *mut u8, VMM_PAGE_SIZE as usize);
        *PAGE_DIRECTORY.add(pd_idx) = pt_phys | PAGE_PRESENT | PAGE_WRITE | PAGE_USER;
    }

    let pt = ((*PAGE_DIRECTORY.add(pd_idx)) & 0xFFFFF000) as *mut u32;
    *pt.add(pt_idx) = (phys & 0xFFFFF000) | (flags & 0xFFF);

    invlpg(virt);
}

pub unsafe fn unmap_page(virt: u32) {
    let pd_idx = (virt >> 22) as usize;
    let pt_idx = ((virt >> 12) & 0x3FF) as usize;

    if (*PAGE_DIRECTORY.add(pd_idx)) & PAGE_PRESENT == 0 {
        return;
    }

    let pt = ((*PAGE_DIRECTORY.add(pd_idx)) & 0xFFFFF000) as *mut u32;
    *pt.add(pt_idx) = 0;

    invlpg(virt);
}

pub unsafe fn get_physical(virt: u32) -> u32 {
    let pd_idx = (virt >> 22) as usize;
    let pt_idx = ((virt >> 12) & 0x3FF) as usize;
    let offset = virt & 0xFFF;

    if (*PAGE_DIRECTORY.add(pd_idx)) & PAGE_PRESENT == 0 {
        return 0;
    }

    let pt = ((*PAGE_DIRECTORY.add(pd_idx)) & 0xFFFFF000) as *mut u32;

    if (*pt.add(pt_idx)) & PAGE_PRESENT == 0 {
        return 0;
    }

    ((*pt.add(pt_idx)) & 0xFFFFF000) + offset
}
