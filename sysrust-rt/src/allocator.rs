//! Global allocator backed by sys_brk with free-list management.
//!
//! Free-list allocator that supports both allocation and deallocation.
//! Uses first-fit search with block splitting and coalescing of adjacent
//! free blocks on deallocation. Grows the heap via brk() when needed.

use core::alloc::{GlobalAlloc, Layout};
use core::ptr;
use crate::sys;

/// Block header stored immediately before every allocated/free region.
///
/// Layout in memory: [BlockHeader][usable bytes ...]
/// The `size` field is the usable size (not including the header itself).
#[repr(C)]
struct BlockHeader {
    size: usize,
    is_free: bool,
    next: *mut BlockHeader,
}

const HEADER_SIZE: usize = core::mem::size_of::<BlockHeader>();
const HEADER_ALIGN: usize = core::mem::align_of::<BlockHeader>();
const MIN_USABLE: usize = 16;
const PAGE_SIZE: usize = 4096;

/// Head of the free list (linked list of all blocks, both free and used).
static mut HEAD: *mut BlockHeader = ptr::null_mut();
/// Current program break (end of heap).
static mut HEAP_END: u64 = 0;
/// Simple lock flag (single-threaded, but guard against reentrancy).
static mut LOCKED: bool = false;

pub struct BrkAllocator;

/// Align `addr` up to `align` (must be a power of two).
#[inline]
const fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}

/// Initialize the heap: call brk(0) to find the current break, then extend
/// it by `initial_size` bytes (page-aligned).
unsafe fn heap_init(initial_size: usize) {
    let current = sys::brk(0);
    // Page-align the start
    let start = (current + 0xFFF) & !0xFFF;
    let size = align_up(initial_size, PAGE_SIZE);
    let new_end = start + size as u64;
    let result = sys::brk(new_end);
    if result < new_end {
        // Cannot initialize heap at all
        return;
    }
    HEAP_END = new_end;

    // Create a single large free block spanning the entire initial heap
    let block = start as *mut BlockHeader;
    (*block).size = size - HEADER_SIZE;
    (*block).is_free = true;
    (*block).next = ptr::null_mut();
    HEAD = block;
}

/// Grow the heap by at least `min_bytes` (including header).
/// Returns a pointer to a new free BlockHeader, or null on failure.
unsafe fn heap_grow(min_bytes: usize) -> *mut BlockHeader {
    let total = align_up(min_bytes + HEADER_SIZE, PAGE_SIZE);
    let new_end = HEAP_END + total as u64;
    let result = sys::brk(new_end);
    if result < new_end {
        return ptr::null_mut();
    }

    let block = HEAP_END as *mut BlockHeader;
    (*block).size = total - HEADER_SIZE;
    (*block).is_free = true;
    (*block).next = ptr::null_mut();
    HEAP_END = new_end;

    // Append to end of block list
    if HEAD.is_null() {
        HEAD = block;
    } else {
        let mut cur = HEAD;
        while !(*cur).next.is_null() {
            cur = (*cur).next;
        }
        (*cur).next = block;

        // If the previous block is free and adjacent, coalesce immediately
        let cur_end = (cur as *mut u8).add(HEADER_SIZE + (*cur).size);
        if cur_end == block as *mut u8 && (*cur).is_free {
            (*cur).size += HEADER_SIZE + (*block).size;
            (*cur).next = (*block).next;
            return cur;
        }
    }

    block
}

/// Find a free block that can satisfy the given layout.
/// Uses first-fit strategy. Alignment padding is absorbed into the block --
/// the caller gets an aligned pointer, and the padding bytes before it are
/// wasted but accounted for in the block's size.
unsafe fn find_free_block(size: usize, align: usize) -> *mut BlockHeader {
    let mut cur = HEAD;
    while !cur.is_null() {
        if (*cur).is_free {
            let data_start = (cur as usize) + HEADER_SIZE;
            let aligned_start = align_up(data_start, align);
            let padding = aligned_start - data_start;
            let needed = size + padding;

            if (*cur).size >= needed {
                return cur;
            }
        }
        cur = (*cur).next;
    }
    ptr::null_mut()
}

/// Split `block` if the remaining space after `needed` bytes is large enough
/// to hold another block (header + MIN_USABLE).
unsafe fn split_block(block: *mut BlockHeader, needed: usize) {
    let remaining = (*block).size - needed;
    if remaining >= HEADER_SIZE + MIN_USABLE {
        let new_block = (block as *mut u8).add(HEADER_SIZE + needed) as *mut BlockHeader;
        (*new_block).size = remaining - HEADER_SIZE;
        (*new_block).is_free = true;
        (*new_block).next = (*block).next;
        (*block).size = needed;
        (*block).next = new_block;
    }
}

/// Coalesce `block` with its next neighbor if both are free and adjacent in memory.
unsafe fn coalesce(block: *mut BlockHeader) {
    let next = (*block).next;
    if next.is_null() || !(*next).is_free {
        return;
    }
    // Check physical adjacency
    let block_end = (block as *mut u8).add(HEADER_SIZE + (*block).size);
    if block_end == next as *mut u8 {
        (*block).size += HEADER_SIZE + (*next).size;
        (*block).next = (*next).next;
    }
}

unsafe impl GlobalAlloc for BrkAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // Reentrancy guard
        if LOCKED {
            return ptr::null_mut();
        }
        LOCKED = true;

        // Ensure minimum alignment covers header alignment
        let align = layout.align().max(HEADER_ALIGN);
        let size = layout.size().max(MIN_USABLE);

        // Initialize heap on first allocation
        if HEAD.is_null() {
            // Start with 64KB
            heap_init(65536);
            if HEAD.is_null() {
                LOCKED = false;
                return ptr::null_mut();
            }
        }

        // Search free list
        let mut block = find_free_block(size, align);

        // If no block found, grow heap
        if block.is_null() {
            block = heap_grow(size);
            if block.is_null() {
                LOCKED = false;
                return ptr::null_mut();
            }
        }

        // Calculate alignment padding
        let data_start = (block as usize) + HEADER_SIZE;
        let aligned_start = align_up(data_start, align);
        let padding = aligned_start - data_start;
        let needed = size + padding;

        // Split if worthwhile
        split_block(block, needed);

        (*block).is_free = false;

        LOCKED = false;
        aligned_start as *mut u8
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        if ptr.is_null() {
            return;
        }

        if LOCKED {
            // If locked (reentrancy), just leak. Should not happen in practice.
            return;
        }
        LOCKED = true;

        // Walk the block list to find the block that owns this pointer.
        // The pointer may be offset from the block's data start due to alignment,
        // so we find the block whose data range contains ptr.
        let mut cur = HEAD;
        while !cur.is_null() {
            let data_start = (cur as *mut u8).add(HEADER_SIZE);
            let data_end = data_start.add((*cur).size);
            if ptr >= data_start && ptr < data_end {
                // Found the block
                (*cur).is_free = true;

                // Coalesce with next block
                coalesce(cur);

                // Also try to coalesce the previous block with this one
                // by walking from head
                let mut prev = HEAD;
                while !prev.is_null() {
                    if (*prev).is_free && (*prev).next == cur {
                        coalesce(prev);
                        break;
                    }
                    if (*prev).next == cur {
                        break;
                    }
                    prev = (*prev).next;
                }

                LOCKED = false;
                return;
            }
            cur = (*cur).next;
        }

        // Block not found -- this is a bug in the caller, but don't crash.
        LOCKED = false;
    }
}
