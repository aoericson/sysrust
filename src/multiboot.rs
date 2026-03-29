// multiboot.rs -- Multiboot info structures.
//
// Defines the structures passed by a Multiboot-compliant bootloader (GRUB/QEMU)
// to the kernel at boot. We use these to discover the physical memory map.

pub const MULTIBOOT_MAGIC: u32 = 0x2BADB002;
pub const MULTIBOOT_FLAG_MODS: u32 = 1 << 3;
pub const MULTIBOOT_FLAG_MMAP: u32 = 1 << 6;

/// Memory map entry as provided by GRUB.
///
/// The `size` field gives the size of the rest of the entry (not including
/// the size field itself). To iterate entries, advance by entry.size + 4.
#[repr(C, packed)]
pub struct MmapEntry {
    pub size: u32,
    pub base_low: u32,
    pub base_high: u32,
    pub length_low: u32,
    pub length_high: u32,
    pub entry_type: u32, // 1 = available RAM, anything else = reserved
}

/// Module entry as provided by the bootloader.
#[repr(C, packed)]
pub struct ModEntry {
    pub mod_start: u32,
    pub mod_end: u32,
    pub cmdline: u32,
    pub padding: u32,
}

/// Multiboot info structure (fields up to mmap_addr).
#[repr(C, packed)]
pub struct MultibootInfo {
    pub flags: u32,
    pub mem_lower: u32,
    pub mem_upper: u32,
    pub boot_device: u32,
    pub cmdline: u32,
    pub mods_count: u32,
    pub mods_addr: u32,
    pub syms: [u32; 4],
    pub mmap_length: u32,
    pub mmap_addr: u32,
}
