// pmm.rs -- Bitmap-based physical page allocator.
//
// Uses a static bitmap where each bit represents one 4KB physical page.
// Bit set (1) = page is used/reserved, bit clear (0) = page is free.
//
// Initialization:
//   1. Mark all pages as reserved (all bits set).
//   2. Walk the Multiboot memory map and mark type-1 (available) regions free.
//   3. Re-reserve the first 1MB (BIOS, VGA, legacy hardware).
//   4. Re-reserve the kernel image (0x100000 to __bss_end, rounded up).

use crate::multiboot::{MultibootInfo, MmapEntry, MULTIBOOT_FLAG_MMAP};

pub const PAGE_SIZE: u32 = 4096;

const MAX_PAGES: u32 = 131072; // support up to 512MB

static mut BITMAP: [u32; (MAX_PAGES / 32) as usize] = [0; (MAX_PAGES / 32) as usize];
static mut TOTAL_PAGES: u32 = 0;
static mut FREE_PAGES: u32 = 0;

// Linker-provided symbol marking the end of the kernel image
unsafe extern "C" {
    static __bss_end: u32;
}

// ---- internal helpers ------------------------------------------------------

fn bitmap_set(page: u32) {
    unsafe {
        BITMAP[(page / 32) as usize] |= 1u32 << (page % 32);
    }
}

fn bitmap_clear(page: u32) {
    unsafe {
        BITMAP[(page / 32) as usize] &= !(1u32 << (page % 32));
    }
}

fn bitmap_test(page: u32) -> bool {
    unsafe { (BITMAP[(page / 32) as usize] & (1u32 << (page % 32))) != 0 }
}

fn mark_region_free(base: u32, length: u32) {
    let page_start = base / PAGE_SIZE;
    let mut page_end = (base + length) / PAGE_SIZE;

    if page_start >= MAX_PAGES {
        return;
    }
    if page_end > MAX_PAGES {
        page_end = MAX_PAGES;
    }

    for p in page_start..page_end {
        if bitmap_test(p) {
            bitmap_clear(p);
            unsafe {
                FREE_PAGES += 1;
            }
        }
    }
}

fn mark_region_used(base: u32, length: u32) {
    let page_start = base / PAGE_SIZE;
    // Round up to cover partial pages
    let mut page_end = (base + length + PAGE_SIZE - 1) / PAGE_SIZE;

    if page_start >= MAX_PAGES {
        return;
    }
    if page_end > MAX_PAGES {
        page_end = MAX_PAGES;
    }

    for p in page_start..page_end {
        if !bitmap_test(p) {
            bitmap_set(p);
            unsafe {
                FREE_PAGES -= 1;
            }
        }
    }
}

// ---- public API ------------------------------------------------------------

pub unsafe fn init(mb_info: &MultibootInfo) {
    let kernel_start: u32 = 0x100000;
    let kernel_end: u32 = &__bss_end as *const u32 as u32;

    // Step 1: mark everything as used
    for i in 0..(MAX_PAGES / 32) as usize {
        BITMAP[i] = 0xFFFF_FFFF;
    }
    TOTAL_PAGES = 0;
    FREE_PAGES = 0;

    // Step 2: parse the Multiboot memory map and free available regions
    if mb_info.flags & MULTIBOOT_FLAG_MMAP != 0 {
        let mut offset: u32 = 0;
        while offset < mb_info.mmap_length {
            let entry = &*((mb_info.mmap_addr + offset) as *const MmapEntry);

            if entry.entry_type == 1 && entry.base_high == 0 {
                let base = entry.base_low;
                let length = entry.length_low;

                // Clamp to MAX_PAGES worth of memory
                let mut end = base + length;
                if end > MAX_PAGES * PAGE_SIZE {
                    end = MAX_PAGES * PAGE_SIZE;
                }
                if base < end {
                    mark_region_free(base, end - base);
                }
            }

            offset += entry.size + 4;
        }
    }

    // Record total pages = all pages we marked free (before reserving)
    TOTAL_PAGES = FREE_PAGES;

    // Step 3: reserve the first 1MB (pages 0 - 255)
    mark_region_used(0, 0x100000);

    // Step 4: reserve the kernel image
    let kernel_size = kernel_end - kernel_start;
    mark_region_used(kernel_start, kernel_size);

    // Update total to include all pages we know about
    TOTAL_PAGES = FREE_PAGES;
    for i in 0..(MAX_PAGES / 32) as usize {
        let word = BITMAP[i];
        for bit in 0..32u32 {
            if word & (1u32 << bit) != 0 {
                TOTAL_PAGES += 1;
            }
        }
    }
}

pub fn reserve_range(start: u32, end: u32) {
    if end <= start {
        return;
    }
    let length = end - start;
    mark_region_used(start, length);
}

pub fn alloc_page() -> u32 {
    unsafe {
        for i in 0..(MAX_PAGES / 32) as usize {
            if BITMAP[i] != 0xFFFF_FFFF {
                for bit in 0..32u32 {
                    if (BITMAP[i] & (1u32 << bit)) == 0 {
                        BITMAP[i] |= 1u32 << bit;
                        FREE_PAGES -= 1;
                        return (i as u32 * 32 + bit) * PAGE_SIZE;
                    }
                }
            }
        }
    }
    0 // out of memory
}

pub fn free_page(addr: u32) {
    // Guard against freeing the null/OOM sentinel address
    if addr == 0 {
        return;
    }
    let page = addr / PAGE_SIZE;
    if page >= MAX_PAGES {
        return;
    }
    if bitmap_test(page) {
        bitmap_clear(page);
        unsafe {
            FREE_PAGES += 1;
        }
    }
}

pub fn get_total_pages() -> u32 {
    unsafe { TOTAL_PAGES }
}

pub fn get_free_pages() -> u32 {
    unsafe { FREE_PAGES }
}
