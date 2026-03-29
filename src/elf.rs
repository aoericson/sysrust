// elf.rs -- ELF32 loader for static executables.
//
// Parses ELF32 headers, maps PT_LOAD segments into virtual memory,
// and returns the entry point address. Programs run in kernel mode
// (ring 0) with access to the full address space.

use crate::pmm;
use crate::string;
use crate::vga;
use crate::vmm;

// ELF magic bytes
const ELF_MAG0: u8 = 0x7F;
const ELF_MAG1: u8 = b'E';
const ELF_MAG2: u8 = b'L';
const ELF_MAG3: u8 = b'F';

// ELF identification indices
const EI_CLASS: usize = 4;
const EI_DATA: usize = 5;

const ELFCLASS32: u8 = 1;
const ELFDATA2LSB: u8 = 1;

// ELF types
const ET_EXEC: u16 = 2;
const EM_386: u16 = 3;

// Program header types
const PT_LOAD: u32 = 1;

/// ELF32 file header.
#[repr(C)]
struct Elf32Ehdr {
    e_ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u32,
    e_phoff: u32,
    e_shoff: u32,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

/// ELF32 program header (segment descriptor).
#[repr(C)]
struct Elf32Phdr {
    p_type: u32,
    p_offset: u32,
    p_vaddr: u32,
    p_paddr: u32,
    p_filesz: u32,
    p_memsz: u32,
    p_flags: u32,
    p_align: u32,
}

/// Information returned after loading an ELF binary.
pub struct ElfInfo {
    pub entry: u32,
    pub brk: u32,
}

/// Check if a buffer starts with the ELF magic number.
pub unsafe fn is_elf(data: *const u8) -> bool {
    *data == ELF_MAG0
        && *data.add(1) == ELF_MAG1
        && *data.add(2) == ELF_MAG2
        && *data.add(3) == ELF_MAG3
}

/// Load a static ELF32 executable into memory.
///
/// Reads PT_LOAD segments, allocates physical pages, maps them at the
/// requested virtual addresses, copies file data, and zeroes BSS.
///
/// Returns the entry point and initial program break on success.
pub unsafe fn load(data: *const u8, size: u32) -> Result<ElfInfo, &'static str> {
    if size < core::mem::size_of::<Elf32Ehdr>() as u32 {
        return Err("ELF: file too small");
    }

    let ehdr = data as *const Elf32Ehdr;

    // Validate magic
    if (*ehdr).e_ident[0] != ELF_MAG0
        || (*ehdr).e_ident[1] != ELF_MAG1
        || (*ehdr).e_ident[2] != ELF_MAG2
        || (*ehdr).e_ident[3] != ELF_MAG3
    {
        return Err("ELF: bad magic");
    }

    // Validate class and encoding
    if (*ehdr).e_ident[EI_CLASS] != ELFCLASS32 {
        return Err("ELF: not 32-bit");
    }
    if (*ehdr).e_ident[EI_DATA] != ELFDATA2LSB {
        return Err("ELF: not little-endian");
    }

    // Validate type and machine
    if (*ehdr).e_type != ET_EXEC {
        return Err("ELF: not executable");
    }
    if (*ehdr).e_machine != EM_386 {
        return Err("ELF: not i386");
    }

    let entry = (*ehdr).e_entry;
    let phoff = (*ehdr).e_phoff;
    let phnum = (*ehdr).e_phnum as u32;
    let phentsize = (*ehdr).e_phentsize as u32;

    if phnum == 0 {
        return Err("ELF: no program headers");
    }

    let mut highest_addr: u32 = 0;
    let mut segments_loaded: u32 = 0;

    // Process each program header
    for i in 0..phnum {
        let phdr = data.add((phoff + i * phentsize) as usize) as *const Elf32Phdr;

        if (*phdr).p_type != PT_LOAD {
            continue;
        }

        let vaddr = (*phdr).p_vaddr;
        let memsz = (*phdr).p_memsz;
        let filesz = (*phdr).p_filesz;
        let offset = (*phdr).p_offset;

        if memsz == 0 {
            continue;
        }

        // Validate offset + filesz doesn't exceed file
        if offset + filesz > size {
            return Err("ELF: segment extends past file");
        }

        // Allocate and map pages for this segment
        let start_page = vaddr & !0xFFF;
        let end_page = (vaddr + memsz + 0xFFF) & !0xFFF;
        let flags = vmm::PAGE_PRESENT | vmm::PAGE_WRITE;

        let mut page = start_page;
        while page < end_page {
            // Only map if not already mapped (segments may overlap pages)
            if vmm::get_physical(page) == 0 {
                let phys = pmm::alloc_page();
                if phys == 0 {
                    return Err("ELF: out of memory");
                }
                vmm::map_page(page, phys, flags);
                // Zero the fresh page
                string::memset(page as *mut u8, 0, 0x1000);
            }
            page += 0x1000;
        }

        // Copy file data into mapped memory
        if filesz > 0 {
            string::memcpy(
                vaddr as *mut u8,
                data.add(offset as usize),
                filesz as usize,
            );
        }

        // BSS is already zeroed (we zeroed fresh pages above)

        let seg_end = vaddr + memsz;
        if seg_end > highest_addr {
            highest_addr = seg_end;
        }

        segments_loaded += 1;
    }

    if segments_loaded == 0 {
        return Err("ELF: no loadable segments");
    }

    Ok(ElfInfo {
        entry,
        brk: (highest_addr + 0xFFF) & !0xFFF,
    })
}
