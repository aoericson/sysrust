// gdt.rs -- Global Descriptor Table setup for x86_64 long mode.
//
// The GDT is an x86 structure that defines memory segments. In 64-bit long mode,
// segmentation is largely disabled -- the CPU ignores base/limit for CS and DS.
// However, the GDT must still exist with valid code and data descriptors so that
// the segment registers hold valid selectors.
//
// GDT entries (selectors):
//   0x00 = null descriptor (required by x86)
//   0x08 = kernel code segment (execute + read, ring 0, L=1 for 64-bit)
//   0x10 = kernel data segment (read + write, ring 0)

use core::mem::size_of;

/// A single GDT entry (8 bytes). The base address and limit are each split
/// across non-contiguous bit fields -- a historical artifact of the 286/386
/// transition. In 64-bit mode the CPU ignores base/limit for code/data, but
/// the encoding is still present.
#[repr(C, packed)]
struct GdtEntry {
    limit_low:   u16,   // limit bits 0-15
    base_low:    u16,   // base bits 0-15
    base_mid:    u8,    // base bits 16-23
    access:      u8,    // access flags (present, ring, type)
    granularity: u8,    // limit bits 16-19 (low nibble) + flags (high nibble)
    base_high:   u8,    // base bits 24-31
}

/// Pointer structure loaded by the lgdt instruction.
/// In 64-bit mode the base field is 8 bytes.
#[repr(C, packed)]
struct GdtPtr {
    limit: u16,   // total size of GDT in bytes, minus 1
    base:  u64,   // linear address of the GDT array
}

static mut GDT: [GdtEntry; 3] = [
    GdtEntry { limit_low: 0, base_low: 0, base_mid: 0, access: 0, granularity: 0, base_high: 0 },
    GdtEntry { limit_low: 0, base_low: 0, base_mid: 0, access: 0, granularity: 0, base_high: 0 },
    GdtEntry { limit_low: 0, base_low: 0, base_mid: 0, access: 0, granularity: 0, base_high: 0 },
];

static mut GP: GdtPtr = GdtPtr { limit: 0, base: 0 };

// Defined in boot.s -- loads the GDT and reloads all segment registers.
// In x86_64 System V ABI the first argument is passed in RDI.
unsafe extern "C" {
    fn gdt_flush(gdt_ptr_addr: u64);
}

/// Pack a GDT entry from human-readable parameters.
///
/// access byte bits: P(1) DPL(2) S(1) Type(4)
///   0x9A = 1 00 1 1010 = present, ring 0, code, execute/read
///   0x92 = 1 00 1 0010 = present, ring 0, data, read/write
///
/// granularity byte high nibble for 64-bit code:
///   0xA = G(1) L(1) D(0) AVL(0) = 4KB granularity, long mode, 32-bit opsize off
/// granularity byte high nibble for data:
///   0xC = G(1) D(1) L(0) AVL(0) = 4KB granularity, 32-bit operand size
unsafe fn gdt_set_entry(i: usize, base: u32, limit: u32, access: u8, gran: u8) {
    GDT[i].base_low    = (base & 0xFFFF) as u16;
    GDT[i].base_mid    = ((base >> 16) & 0xFF) as u8;
    GDT[i].base_high   = ((base >> 24) & 0xFF) as u8;
    GDT[i].limit_low   = (limit & 0xFFFF) as u16;
    GDT[i].granularity  = (((limit >> 16) & 0x0F) as u8) | (gran & 0xF0);
    GDT[i].access      = access;
}

/// Build and load the GDT for 64-bit long mode.
pub unsafe fn init() {
    GP.limit = (size_of::<[GdtEntry; 3]>() - 1) as u16;
    GP.base  = &raw const GDT as u64;

    gdt_set_entry(0, 0, 0, 0, 0);                        // null descriptor
    gdt_set_entry(1, 0, 0xFFFFFFFF, 0x9A, 0xAF);         // kernel code: L=1, D=0
    gdt_set_entry(2, 0, 0xFFFFFFFF, 0x92, 0xCF);         // kernel data: D=1

    gdt_flush(&raw const GP as u64);  // load GDT + reload segment registers
}
