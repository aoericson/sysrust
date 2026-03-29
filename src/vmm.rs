// vmm.rs -- Virtual memory manager (x86 paging).
//
// Two-level page translation:
//   Page Directory (1024 entries) -> Page Tables (1024 entries each) -> 4KB pages
//
// On init we identity-map the first 128MB so that phys == virt for the
// kernel, heap, DMA regions, page tables, and loaded ELF programs.

use core::arch::asm;
use crate::pmm;

pub const VMM_PAGE_SIZE: u32 = 4096;

// Page flags
pub const PAGE_PRESENT: u32 = 0x01;
pub const PAGE_WRITE: u32 = 0x02;
pub const PAGE_USER: u32 = 0x04;

// Number of 4MB regions to identity-map at boot (32 * 4MB = 128MB)
const IDENTITY_MAP_ENTRIES: u32 = 32;

// ---- private state ---------------------------------------------------------

static mut PAGE_DIRECTORY: *mut u32 = core::ptr::null_mut();
static mut PD_PHYS: u32 = 0;

// ---- helpers ---------------------------------------------------------------

#[inline]
unsafe fn invlpg(virt: u32) {
    asm!("invlpg [{}]", in(reg) virt, options(nostack, preserves_flags));
}

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
    PAGE_DIRECTORY = PD_PHYS as *mut u32;
    zero_page(PAGE_DIRECTORY as *mut u8, VMM_PAGE_SIZE as usize);

    // Identity-map the first 128MB (32 page-directory entries, 32 page tables).
    for i in 0u32..IDENTITY_MAP_ENTRIES {
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
        if pt_phys == 0 {
            crate::vga::puts(b"vmm: out of memory for page table\n");
            return;
        }
        zero_page(pt_phys as *mut u8, VMM_PAGE_SIZE as usize);
        *PAGE_DIRECTORY.add(pd_idx) = pt_phys | PAGE_PRESENT | PAGE_WRITE | PAGE_USER;
    }

    let pt = ((*PAGE_DIRECTORY.add(pd_idx)) & 0xFFFFF000) as *mut u32;
    *pt.add(pt_idx) = (phys & 0xFFFFF000) | (flags & 0xFFF);

    invlpg(virt);
}

/// Map a contiguous range of virtual to physical pages.
pub unsafe fn map_range(virt_start: u32, phys_start: u32, size: u32, flags: u32) {
    let pages = (size + VMM_PAGE_SIZE - 1) / VMM_PAGE_SIZE;
    for i in 0..pages {
        map_page(
            virt_start + i * VMM_PAGE_SIZE,
            phys_start + i * VMM_PAGE_SIZE,
            flags,
        );
    }
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
