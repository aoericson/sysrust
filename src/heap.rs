// heap.rs -- Kernel heap allocator.
//
// Free-list allocator with boundary tags (header before each block).
// Supports first-fit allocation, block splitting, and forward coalescing.
//
// Layout:
//   [BlockHeader][...usable bytes...]  [BlockHeader][...usable bytes...]  ...
//
// The heap lives within the first 16MB identity-mapped region so no
// additional VMM mappings are required at runtime.  Physical pages are
// allocated from the PMM and mapped explicitly via the VMM on expansion.
//
// HEAP_START         0x00500000  (5 MB -- well above the kernel image)
// HEAP_INITIAL_PAGES 256        (1 MB initial heap)
// HEAP_MAX_PAGES     16384      (64 MB ceiling, within 128 MB identity map)

use crate::pmm;
use crate::sync::Spinlock;
use crate::vmm;

static mut HEAP_LOCK: Spinlock = Spinlock::new();

// ---- tunables --------------------------------------------------------------

const HEAP_START: u32 = 0x0050_0000;
const HEAP_INITIAL_PAGES: u32 = 256;  // 1 MB
const HEAP_MAX_PAGES: u32 = 16384;    // 64 MB
const MIN_SPLIT: u32 = 16;            // don't split if remainder < this

// ---- block header ----------------------------------------------------------

// Each allocation is preceded by a BlockHeader. The struct is padded to
// 12 bytes so that the data area that follows is always 4-byte-aligned
// (assuming HEAP_START is 4-byte-aligned, which it is at 5 MB).
//
// Field layout (32-bit x86):
//   offset 0  : size     (4 bytes) -- usable bytes, NOT including header
//   offset 4  : is_free  (4 bytes) -- 1 = free, 0 = allocated
//   offset 8  : next     (4 bytes) -- next BlockHeader* in the flat list
#[repr(C)]
struct BlockHeader {
    size: u32,
    is_free: u32,
    next: *mut BlockHeader,
}

const HEADER_SIZE: u32 = core::mem::size_of::<BlockHeader>() as u32;

// ---- module state ----------------------------------------------------------

static mut HEAP_HEAD: *mut BlockHeader = core::ptr::null_mut();
static mut HEAP_PAGES: u32 = 0;

// ---- private helpers -------------------------------------------------------

/// expand_heap -- allocate `pages` more physical pages from the PMM and
/// map them at the current heap top, then append a free block covering
/// the new space.
///
/// Returns true on success, false if the PMM is exhausted or the page
/// ceiling would be exceeded.
unsafe fn expand_heap(pages: u32) -> bool {
    if HEAP_PAGES + pages > HEAP_MAX_PAGES {
        return false;
    }

    let base_virt = HEAP_START + HEAP_PAGES * vmm::VMM_PAGE_SIZE;
    let mut mapped: u32 = 0;

    for i in 0..pages {
        let phys = pmm::alloc_page();
        let virt = base_virt + i * vmm::VMM_PAGE_SIZE;

        if phys == 0 {
            break; // PMM exhausted -- use whatever pages we got
        }

        vmm::map_page(virt, phys, vmm::PAGE_PRESENT | vmm::PAGE_WRITE);
        HEAP_PAGES += 1;
        mapped += 1;
    }

    // If no pages were mapped at all, nothing to do
    if mapped == 0 {
        return false;
    }

    // Carve a single free block out of the newly mapped region
    let blk = base_virt as *mut BlockHeader;
    (*blk).size = mapped * vmm::VMM_PAGE_SIZE - HEADER_SIZE;
    (*blk).is_free = 1;
    (*blk).next = core::ptr::null_mut();

    // Append it to the tail of the block list
    if HEAP_HEAD.is_null() {
        HEAP_HEAD = blk;
    } else {
        let mut last = HEAP_HEAD;
        while !(*last).next.is_null() {
            last = (*last).next;
        }
        (*last).next = blk;
    }

    true
}

/// coalesce -- walk the entire list and merge any adjacent free blocks.
/// O(n^2) in the worst case but acceptable for a kernel heap of this size.
unsafe fn coalesce() {
    let mut cur = HEAP_HEAD;

    while !cur.is_null() {
        if (*cur).is_free != 0 && !(*cur).next.is_null() && (*(*cur).next).is_free != 0 {
            // Check physical adjacency: the next header must sit exactly
            // HEADER_SIZE + cur->size bytes after cur.
            let expected_next = (cur as *mut u8).add((HEADER_SIZE + (*cur).size) as usize);
            if expected_next as *mut BlockHeader == (*cur).next {
                // Merge: absorb next into cur
                (*cur).size += HEADER_SIZE + (*(*cur).next).size;
                (*cur).next = (*(*cur).next).next;
                // Don't advance cur -- keep trying to merge further
                continue;
            }
        }
        cur = (*cur).next;
    }
}

/// Try to allocate from a given block using first-fit with optional splitting.
/// Returns a pointer to the usable region, or null if the block doesn't fit.
unsafe fn try_alloc(cur: *mut BlockHeader, aligned_size: u32) -> *mut u8 {
    if (*cur).is_free != 0 && (*cur).size >= aligned_size {
        // Split if there is enough room for a header + MIN_SPLIT bytes
        if (*cur).size >= aligned_size + HEADER_SIZE + MIN_SPLIT {
            let split = (cur as *mut u8).add((HEADER_SIZE + aligned_size) as usize)
                as *mut BlockHeader;

            (*split).size = (*cur).size - aligned_size - HEADER_SIZE;
            (*split).is_free = 1;
            (*split).next = (*cur).next;

            (*cur).size = aligned_size;
            (*cur).next = split;
        }

        (*cur).is_free = 0;
        return (cur as *mut u8).add(HEADER_SIZE as usize);
    }
    core::ptr::null_mut()
}

// ---- public API ------------------------------------------------------------

pub unsafe fn init() {
    HEAP_HEAD = core::ptr::null_mut();
    HEAP_PAGES = 0;
    expand_heap(HEAP_INITIAL_PAGES);
}

pub unsafe fn kmalloc(size: usize) -> *mut u8 {
    if size == 0 {
        return core::ptr::null_mut();
    }

    HEAP_LOCK.lock();

    // Round up to 4-byte alignment
    let aligned_size = ((size as u32) + 3) & !3;

    // First-fit search
    let mut cur = HEAP_HEAD;
    while !cur.is_null() {
        let result = try_alloc(cur, aligned_size);
        if !result.is_null() {
            HEAP_LOCK.unlock();
            return result;
        }
        cur = (*cur).next;
    }

    // No suitable block found -- try to grow the heap
    let mut need_pages = (aligned_size + HEADER_SIZE + vmm::VMM_PAGE_SIZE - 1)
        / vmm::VMM_PAGE_SIZE;
    if need_pages < 16 {
        need_pages = 16; // grow by at least 64 KB at a time
    }

    if !expand_heap(need_pages) {
        HEAP_LOCK.unlock();
        return core::ptr::null_mut(); // truly out of memory
    }

    // Retry after expansion (tail block is now free)
    cur = HEAP_HEAD;
    while !cur.is_null() {
        let result = try_alloc(cur, aligned_size);
        if !result.is_null() {
            HEAP_LOCK.unlock();
            return result;
        }
        cur = (*cur).next;
    }

    HEAP_LOCK.unlock();
    core::ptr::null_mut()
}

pub unsafe fn kfree(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }

    HEAP_LOCK.lock();

    let hdr = ptr.sub(HEADER_SIZE as usize) as *mut BlockHeader;
    // Guard against double-free
    if (*hdr).is_free != 0 {
        HEAP_LOCK.unlock();
        return;
    }
    (*hdr).is_free = 1;

    coalesce();
    HEAP_LOCK.unlock();
}

pub unsafe fn get_free() -> u32 {
    let mut cur = HEAP_HEAD;
    let mut free_bytes: u32 = 0;

    while !cur.is_null() {
        if (*cur).is_free != 0 {
            free_bytes += (*cur).size;
        }
        cur = (*cur).next;
    }

    free_bytes
}
