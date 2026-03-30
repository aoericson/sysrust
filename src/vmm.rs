// vmm.rs -- Virtual memory manager (x86_64 4-level paging).
//
// Four-level page translation:
//   PML4 (512 entries) -> PDPT (512 entries) -> PD (512 entries) -> PT (512 entries) -> 4KB pages
//
// Virtual address breakdown (48-bit canonical):
//   [47:39] PML4 index (9 bits)
//   [38:30] PDPT index (9 bits)
//   [29:21] PD index (9 bits)
//   [20:12] PT index (9 bits)
//   [11:0]  Page offset (12 bits)
//
// On init we read CR3 to obtain the PML4 set up by the boot assembly, which
// already identity-maps the first 1GB using 2MB pages. We reuse those bootstrap
// tables for the kernel and only allocate new tables when mapping additional pages.

use core::arch::asm;
use crate::pmm;

pub const VMM_PAGE_SIZE: u64 = 4096;

// Page flags (low 12 bits of a page table entry)
pub const PAGE_PRESENT: u64 = 0x01;
pub const PAGE_WRITE:   u64 = 0x02;
pub const PAGE_USER:    u64 = 0x04;

/// Mask to extract the physical address from a page table entry (bits 12-51).
const ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

// ---- private state ---------------------------------------------------------

/// Physical address of the PML4 table (read from CR3 at init).
static mut PML4_PHYS: u64 = 0;

// ---- helpers ---------------------------------------------------------------

#[inline]
unsafe fn invlpg(virt: u64) {
    asm!("invlpg [{}]", in(reg) virt, options(nostack, preserves_flags));
}

/// Zero a 4KB-aligned page.
unsafe fn zero_page(dest: *mut u8, count: usize) {
    let mut i = 0;
    while i < count {
        *dest.add(i) = 0;
        i += 1;
    }
}

/// Extract the PML4 index (bits 39-47) from a virtual address.
#[inline]
fn pml4_index(virt: u64) -> usize {
    ((virt >> 39) & 0x1FF) as usize
}

/// Extract the PDPT index (bits 30-38) from a virtual address.
#[inline]
fn pdpt_index(virt: u64) -> usize {
    ((virt >> 30) & 0x1FF) as usize
}

/// Extract the PD index (bits 21-29) from a virtual address.
#[inline]
fn pd_index(virt: u64) -> usize {
    ((virt >> 21) & 0x1FF) as usize
}

/// Extract the PT index (bits 12-20) from a virtual address.
#[inline]
fn pt_index(virt: u64) -> usize {
    ((virt >> 12) & 0x1FF) as usize
}

/// Walk one level of the page table hierarchy. If the entry at `table[index]`
/// is not present, allocate a new page table and install it.
/// Returns a pointer to the next-level table, or null on allocation failure.
///
/// SAFETY: `table` must point to a valid, identity-mapped page table.
/// This only works for tables in the identity-mapped first 1GB.
unsafe fn walk_or_alloc(table: *mut u64, index: usize) -> *mut u64 {
    let entry = *table.add(index);
    if entry & PAGE_PRESENT != 0 {
        // Entry exists -- extract physical address and use as pointer
        // (works because the first 1GB is identity-mapped).
        return (entry & ADDR_MASK) as *mut u64;
    }

    // Allocate a new page table
    let new_table_phys = pmm::alloc_page();
    if new_table_phys == 0 {
        crate::vga::puts(b"vmm: out of memory for page table\n");
        return core::ptr::null_mut();
    }
    zero_page(new_table_phys as *mut u8, VMM_PAGE_SIZE as usize);

    // Install in the parent table with present + writable + user flags
    *table.add(index) = new_table_phys | PAGE_PRESENT | PAGE_WRITE | PAGE_USER;

    new_table_phys as *mut u64
}

/// Walk one level of the page table hierarchy (read-only, no allocation).
/// Returns a pointer to the next-level table, or null if not present.
unsafe fn walk_readonly(table: *mut u64, index: usize) -> *mut u64 {
    let entry = *table.add(index);
    if entry & PAGE_PRESENT == 0 {
        return core::ptr::null_mut();
    }
    (entry & ADDR_MASK) as *mut u64
}

// ---- public API ------------------------------------------------------------

/// Initialize the VMM by reading the current PML4 from CR3.
///
/// The boot assembly has already set up identity mapping for the first 1GB
/// using 2MB pages. We reuse those bootstrap page tables.
pub unsafe fn init() {
    let cr3: u64;
    asm!("mov {0}, cr3", out(reg) cr3);
    PML4_PHYS = cr3 & ADDR_MASK;
}

/// Map a single 4KB page: virt -> phys with the given flags.
///
/// Walks all four levels of the page table hierarchy, allocating intermediate
/// tables as needed from the PMM. Newly allocated tables are in low physical
/// memory (identity-mapped first 1GB) and thus directly writable.
pub unsafe fn map_page(virt: u64, phys: u64, flags: u64) {
    let pml4 = PML4_PHYS as *mut u64;

    let pdpt = walk_or_alloc(pml4, pml4_index(virt));
    if pdpt.is_null() { return; }

    let pd = walk_or_alloc(pdpt, pdpt_index(virt));
    if pd.is_null() { return; }

    let pt = walk_or_alloc(pd, pd_index(virt));
    if pt.is_null() { return; }

    // Install the final 4KB page mapping
    *pt.add(pt_index(virt)) = (phys & ADDR_MASK) | (flags & 0xFFF);

    invlpg(virt);
}

/// Map a contiguous range of virtual to physical 4KB pages.
pub unsafe fn map_range(virt_start: u64, phys_start: u64, size: u64, flags: u64) {
    let pages = (size + VMM_PAGE_SIZE - 1) / VMM_PAGE_SIZE;
    let mut i: u64 = 0;
    while i < pages {
        map_page(
            virt_start + i * VMM_PAGE_SIZE,
            phys_start + i * VMM_PAGE_SIZE,
            flags,
        );
        i += 1;
    }
}

/// Unmap a single 4KB page. Zeroes the PT entry and invalidates the TLB.
pub unsafe fn unmap_page(virt: u64) {
    let pml4 = PML4_PHYS as *mut u64;

    let pdpt = walk_readonly(pml4, pml4_index(virt));
    if pdpt.is_null() { return; }

    let pd = walk_readonly(pdpt, pdpt_index(virt));
    if pd.is_null() { return; }

    let pt = walk_readonly(pd, pd_index(virt));
    if pt.is_null() { return; }

    *pt.add(pt_index(virt)) = 0;

    invlpg(virt);
}

/// Translate a virtual address to its physical address.
/// Returns 0 if any level of the page table walk fails.
pub unsafe fn get_physical(virt: u64) -> u64 {
    let pml4 = PML4_PHYS as *mut u64;

    let pdpt = walk_readonly(pml4, pml4_index(virt));
    if pdpt.is_null() { return 0; }

    let pd = walk_readonly(pdpt, pdpt_index(virt));
    if pd.is_null() { return 0; }

    let pt = walk_readonly(pd, pd_index(virt));
    if pt.is_null() { return 0; }

    let pte = *pt.add(pt_index(virt));
    if pte & PAGE_PRESENT == 0 {
        return 0;
    }

    (pte & ADDR_MASK) + (virt & 0xFFF)
}
