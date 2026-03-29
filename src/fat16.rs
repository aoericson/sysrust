// fat16.rs -- FAT16 filesystem driver.
//
// Reads the FAT16 boot sector (BPB), caches the FAT table in memory,
// and provides read/write access to files in the root directory.
//
// Simplifications:
//   - Root directory only (no subdirectories)
//   - 8.3 filenames only (no long file names)
//   - FAT table cached in memory via kmalloc
//   - No file deletion

use crate::ata;
use crate::heap;
use crate::string;
use crate::vga;

/// FAT16 on-disk directory entry (32 bytes).
#[repr(C, packed)]
struct Fat16DirEntry {
    name: [u8; 11],       // 8.3 format, space-padded
    attr: u8,
    reserved: [u8; 10],
    time: u16,
    date: u16,
    first_cluster: u16,
    size: u32,
}

/// Boot Parameter Block -- parsed from sector 0.
static mut BPB: Bpb = Bpb {
    bytes_per_sector: 0,
    sectors_per_cluster: 0,
    reserved_sectors: 0,
    num_fats: 0,
    root_entry_count: 0,
    total_sectors_16: 0,
    total_sectors_32: 0,
    fat_size_16: 0,
    first_fat_sector: 0,
    first_root_sector: 0,
    root_dir_sectors: 0,
    first_data_sector: 0,
    cluster_size: 0,
};

struct Bpb {
    bytes_per_sector: u16,
    sectors_per_cluster: u8,
    reserved_sectors: u16,
    num_fats: u8,
    root_entry_count: u16,
    total_sectors_16: u16,
    total_sectors_32: u32,
    fat_size_16: u16,
    first_fat_sector: u32,
    first_root_sector: u32,
    root_dir_sectors: u32,
    first_data_sector: u32,
    cluster_size: u32, // bytes per cluster
}

static mut FAT_TABLE: *mut u16 = core::ptr::null_mut(); // cached FAT in memory
static mut FAT_ENTRIES: u32 = 0;                         // number of entries in the FAT
static mut FAT16_READY: i32 = 0;                         // 1 after successful init

// Scratch buffer for reading one sector
static mut SECTOR_BUF: [u8; 512] = [0u8; 512];

// ---- 8.3 filename conversion ------------------------------------------------

/// Convert a user-friendly filename like "hello.c" to FAT 8.3 format
/// ("HELLO   C  "). Output buf must be at least 11 bytes.
unsafe fn name_to_83(name: *const u8, out: *mut u8) {
    // Fill with spaces
    for i in 0..11 {
        *out.add(i) = b' ';
    }

    // Copy base name (up to 8 chars, before the dot)
    let mut i: usize = 0;
    let mut j: usize = 0;
    while *name.add(i) != 0 && *name.add(i) != b'.' && j < 8 {
        let mut c = *name.add(i);
        if c >= b'a' && c <= b'z' {
            c = c - b'a' + b'A';
        }
        *out.add(j) = c;
        j += 1;
        i += 1;
    }

    // Skip to extension
    while *name.add(i) != 0 && *name.add(i) != b'.' {
        i += 1;
    }

    // Copy extension (up to 3 chars, after the dot)
    if *name.add(i) == b'.' {
        i += 1;
        j = 8;
        while *name.add(i) != 0 && j < 11 {
            let mut c = *name.add(i);
            if c >= b'a' && c <= b'z' {
                c = c - b'a' + b'A';
            }
            *out.add(j) = c;
            j += 1;
            i += 1;
        }
    }
}

/// Convert an 8.3 directory entry name to a user-friendly string.
/// Example: "HELLO   C  " -> "HELLO.C"
/// Output buf must be at least 13 bytes (8 + '.' + 3 + '\0').
unsafe fn name_from_83(fat_name: *const u8, out: *mut u8) {
    let mut j: usize = 0;

    // Copy base name, trimming trailing spaces
    for i in 0..8 {
        if *fat_name.add(i) != b' ' {
            *out.add(j) = *fat_name.add(i);
            j += 1;
        }
    }

    // Check if there's an extension
    if *fat_name.add(8) != b' ' {
        *out.add(j) = b'.';
        j += 1;
        for i in 8..11 {
            if *fat_name.add(i) != b' ' {
                *out.add(j) = *fat_name.add(i);
                j += 1;
            }
        }
    }

    *out.add(j) = 0;
}

// ---- Sector I/O helpers -----------------------------------------------------

/// Read a cluster's worth of data into buf. Returns 0 on success.
unsafe fn read_cluster(cluster: u32, buf: *mut u8) -> i32 {
    if cluster < 2 {
        return -1;
    }

    let lba = BPB.first_data_sector + (cluster - 2) * BPB.sectors_per_cluster as u32;

    for i in 0..BPB.sectors_per_cluster as u32 {
        if ata::read_sector(lba + i, buf.add((i * 512) as usize)) < 0 {
            return -1;
        }
    }
    0
}

/// Write a cluster's worth of data from buf. Returns 0 on success.
unsafe fn write_cluster(cluster: u32, buf: *const u8) -> i32 {
    if cluster < 2 {
        return -1;
    }

    let lba = BPB.first_data_sector + (cluster - 2) * BPB.sectors_per_cluster as u32;

    for i in 0..BPB.sectors_per_cluster as u32 {
        if ata::write_sector(lba + i, buf.add((i * 512) as usize)) < 0 {
            return -1;
        }
    }
    0
}

// ---- FAT table operations ---------------------------------------------------

/// Get the next cluster in the chain. Returns cluster number,
/// or 0xFFFF+ if end-of-chain.
unsafe fn fat_next(cluster: u16) -> u16 {
    if cluster as u32 >= FAT_ENTRIES {
        return 0xFFFF;
    }
    *FAT_TABLE.add(cluster as usize)
}

/// Find a free cluster in the FAT. Returns cluster number, or 0 if full.
unsafe fn fat_alloc() -> u16 {
    for i in 2..FAT_ENTRIES as u16 {
        if *FAT_TABLE.add(i as usize) == 0x0000 {
            *FAT_TABLE.add(i as usize) = 0xFFFF; // mark as end-of-chain
            return i;
        }
    }
    0 // disk full
}

/// Flush the in-memory FAT table back to disk.
/// Writes to all copies of the FAT.
unsafe fn fat_flush() -> i32 {
    let sectors = BPB.fat_size_16 as u32;
    let fat_raw = FAT_TABLE as *const u8;

    for f in 0..BPB.num_fats as u32 {
        let base = BPB.first_fat_sector + f * BPB.fat_size_16 as u32;
        for s in 0..sectors {
            if ata::write_sector(base + s, fat_raw.add((s * 512) as usize)) < 0 {
                return -1;
            }
        }
    }
    0
}

// ---- Root directory helpers -------------------------------------------------

/// Read a root directory entry by index. Returns 0 on success.
unsafe fn read_root_entry(index: u32, entry: *mut Fat16DirEntry) -> i32 {
    let entry_size = core::mem::size_of::<Fat16DirEntry>() as u32;
    let entries_per_sector = 512 / entry_size;
    let sector_offset = index / entries_per_sector;
    let entry_offset = index % entries_per_sector;

    if ata::read_sector(
        BPB.first_root_sector + sector_offset,
        SECTOR_BUF.as_mut_ptr(),
    ) < 0
    {
        return -1;
    }

    string::memcpy(
        entry as *mut u8,
        SECTOR_BUF.as_ptr().add((entry_offset * entry_size) as usize),
        entry_size as usize,
    );
    0
}

/// Write a root directory entry by index. Returns 0 on success.
unsafe fn write_root_entry(index: u32, entry: *const Fat16DirEntry) -> i32 {
    let entry_size = core::mem::size_of::<Fat16DirEntry>() as u32;
    let entries_per_sector = 512 / entry_size;
    let sector_offset = index / entries_per_sector;
    let entry_offset = index % entries_per_sector;

    // Read-modify-write: read the sector, update the entry, write back
    if ata::read_sector(
        BPB.first_root_sector + sector_offset,
        SECTOR_BUF.as_mut_ptr(),
    ) < 0
    {
        return -1;
    }

    string::memcpy(
        SECTOR_BUF.as_mut_ptr().add((entry_offset * entry_size) as usize),
        entry as *const u8,
        entry_size as usize,
    );

    if ata::write_sector(
        BPB.first_root_sector + sector_offset,
        SECTOR_BUF.as_ptr(),
    ) < 0
    {
        return -1;
    }

    0
}

/// Find a file in the root directory by 8.3 name.
/// Returns the directory entry index, or -1 if not found.
/// If entry_out is not null, the entry is copied there.
unsafe fn find_root_entry(fat_name: *const u8, entry_out: *mut Fat16DirEntry) -> i32 {
    let mut entry: Fat16DirEntry = core::mem::zeroed();

    for i in 0..BPB.root_entry_count as u32 {
        if read_root_entry(i, &mut entry) < 0 {
            return -1;
        }

        // End of directory
        if entry.name[0] == 0x00 {
            return -1;
        }

        // Deleted entry
        if entry.name[0] == 0xE5 {
            continue;
        }

        // Skip LFN entries and volume labels
        if entry.attr == 0x0F || (entry.attr & 0x08) != 0 {
            continue;
        }

        if string::memcmp(entry.name.as_ptr(), fat_name, 11) == 0 {
            if !entry_out.is_null() {
                core::ptr::copy_nonoverlapping(&entry, entry_out, 1);
            }
            return i as i32;
        }
    }
    -1
}

/// Find a free slot in the root directory.
/// Returns the index, or -1 if directory is full.
unsafe fn find_free_root_entry() -> i32 {
    let mut entry: Fat16DirEntry = core::mem::zeroed();

    for i in 0..BPB.root_entry_count as u32 {
        if read_root_entry(i, &mut entry) < 0 {
            return -1;
        }

        if entry.name[0] == 0x00 || entry.name[0] == 0xE5 {
            return i as i32;
        }
    }
    -1
}

// ---- Public API -------------------------------------------------------------

/// Read boot sector, cache FAT; returns 0 on success.
pub unsafe fn init() -> i32 {
    FAT16_READY = 0;

    // Read boot sector
    if ata::read_sector(0, SECTOR_BUF.as_mut_ptr()) < 0 {
        vga::puts(b"FAT16: cannot read boot sector\n");
        return -1;
    }

    // Parse BPB fields from the boot sector
    string::memcpy(
        &mut BPB.bytes_per_sector as *mut u16 as *mut u8,
        SECTOR_BUF.as_ptr().add(11),
        2,
    );
    string::memcpy(
        &mut BPB.sectors_per_cluster as *mut u8,
        SECTOR_BUF.as_ptr().add(13),
        1,
    );
    string::memcpy(
        &mut BPB.reserved_sectors as *mut u16 as *mut u8,
        SECTOR_BUF.as_ptr().add(14),
        2,
    );
    string::memcpy(
        &mut BPB.num_fats as *mut u8,
        SECTOR_BUF.as_ptr().add(16),
        1,
    );
    string::memcpy(
        &mut BPB.root_entry_count as *mut u16 as *mut u8,
        SECTOR_BUF.as_ptr().add(17),
        2,
    );
    string::memcpy(
        &mut BPB.total_sectors_16 as *mut u16 as *mut u8,
        SECTOR_BUF.as_ptr().add(19),
        2,
    );
    string::memcpy(
        &mut BPB.fat_size_16 as *mut u16 as *mut u8,
        SECTOR_BUF.as_ptr().add(22),
        2,
    );
    string::memcpy(
        &mut BPB.total_sectors_32 as *mut u32 as *mut u8,
        SECTOR_BUF.as_ptr().add(32),
        4,
    );

    // Validate basic fields
    if BPB.bytes_per_sector != 512 {
        vga::puts(b"FAT16: unsupported sector size\n");
        return -1;
    }
    if BPB.fat_size_16 == 0 || BPB.num_fats == 0 {
        vga::puts(b"FAT16: invalid BPB\n");
        return -1;
    }

    // Compute derived layout values
    BPB.first_fat_sector = BPB.reserved_sectors as u32;
    BPB.root_dir_sectors =
        (BPB.root_entry_count as u32 * 32 + 511) / 512;
    BPB.first_root_sector =
        BPB.first_fat_sector + BPB.num_fats as u32 * BPB.fat_size_16 as u32;
    BPB.first_data_sector = BPB.first_root_sector + BPB.root_dir_sectors;
    BPB.cluster_size = BPB.sectors_per_cluster as u32 * 512;

    // Cache the FAT table in memory
    let fat_bytes = BPB.fat_size_16 as u32 * 512;
    FAT_ENTRIES = fat_bytes / 2;

    FAT_TABLE = heap::kmalloc(fat_bytes as usize) as *mut u16;
    if FAT_TABLE.is_null() {
        vga::puts(b"FAT16: cannot allocate FAT cache\n");
        return -1;
    }

    let fat_raw = FAT_TABLE as *mut u8;
    for s in 0..BPB.fat_size_16 as u32 {
        if ata::read_sector(
            BPB.first_fat_sector + s,
            fat_raw.add((s * 512) as usize),
        ) < 0
        {
            vga::puts(b"FAT16: cannot read FAT\n");
            heap::kfree(FAT_TABLE as *mut u8);
            FAT_TABLE = core::ptr::null_mut();
            return -1;
        }
    }

    FAT16_READY = 1;
    vga::puts(b"FAT16: mounted\n");
    0
}

/// Print root directory listing. Returns 0 on success.
pub unsafe fn list_files(callback: fn(*const u8, u32)) {
    if FAT16_READY == 0 {
        return;
    }

    let mut entry: Fat16DirEntry = core::mem::zeroed();
    let mut name: [u8; 13] = [0u8; 13];

    for i in 0..BPB.root_entry_count as u32 {
        if read_root_entry(i, &mut entry) < 0 {
            break;
        }

        if entry.name[0] == 0x00 {
            break;
        }
        if entry.name[0] == 0xE5 {
            continue;
        }
        if entry.attr == 0x0F || (entry.attr & 0x08) != 0 {
            continue;
        }
        // Skip subdirectories
        if (entry.attr & 0x10) != 0 {
            continue;
        }

        name_from_83(entry.name.as_ptr(), name.as_mut_ptr());
        callback(name.as_ptr(), entry.size);
    }
}

/// Read a file by name into buf. Returns bytes read, or -1 on error.
pub unsafe fn read_file(name: *const u8, buf: *mut u8, max_size: u32) -> i32 {
    if FAT16_READY == 0 {
        return -1;
    }

    let mut fat_name = [0u8; 11];
    name_to_83(name, fat_name.as_mut_ptr());

    let mut entry: Fat16DirEntry = core::mem::zeroed();
    if find_root_entry(fat_name.as_ptr(), &mut entry) < 0 {
        return -1;
    }

    if entry.size == 0 {
        return 0;
    }

    let mut remaining = entry.size;
    if remaining > max_size {
        remaining = max_size;
    }

    let cluster_buf = heap::kmalloc(BPB.cluster_size as usize);
    if cluster_buf.is_null() {
        return -1;
    }

    let mut cluster = entry.first_cluster;
    let mut offset: u32 = 0;

    while remaining > 0 && cluster >= 2 && cluster < 0xFFF8 {
        if read_cluster(cluster as u32, cluster_buf) < 0 {
            heap::kfree(cluster_buf);
            return -1;
        }

        let mut copy_size = BPB.cluster_size;
        if copy_size > remaining {
            copy_size = remaining;
        }

        string::memcpy(buf.add(offset as usize), cluster_buf, copy_size as usize);
        offset += copy_size;
        remaining -= copy_size;

        cluster = fat_next(cluster);
    }

    heap::kfree(cluster_buf);
    offset as i32
}

/// Write a file by name. Returns 0 on success, -1 on error.
pub unsafe fn write_file(name: *const u8, buf: *const u8, size: u32) -> i32 {
    if FAT16_READY == 0 {
        return -1;
    }

    let mut fat_name = [0u8; 11];
    name_to_83(name, fat_name.as_mut_ptr());

    let mut entry: Fat16DirEntry = core::mem::zeroed();

    // Check if file already exists -- free its cluster chain
    let mut dir_index = find_root_entry(fat_name.as_ptr(), &mut entry);
    if dir_index >= 0 {
        // Free existing cluster chain
        let mut cluster = entry.first_cluster;
        while cluster >= 2 && cluster < 0xFFF8 {
            let next = fat_next(cluster);
            *FAT_TABLE.add(cluster as usize) = 0x0000;
            cluster = next;
        }
    } else {
        // Create a new directory entry
        dir_index = find_free_root_entry();
        if dir_index < 0 {
            return -1;
        }
        string::memset(&mut entry as *mut Fat16DirEntry as *mut u8, 0,
                       core::mem::size_of::<Fat16DirEntry>());
        string::memcpy(entry.name.as_mut_ptr(), fat_name.as_ptr(), 11);
        entry.attr = 0x20; // archive bit
    }

    // Allocate clusters and write data
    let cluster_buf = heap::kmalloc(BPB.cluster_size as usize);
    if cluster_buf.is_null() {
        return -1;
    }

    let mut first_cluster: u16 = 0;
    let mut prev_cluster: u16 = 0;
    let mut offset: u32 = 0;

    while offset < size {
        let cluster = fat_alloc();
        if cluster == 0 {
            heap::kfree(cluster_buf);
            return -1; // disk full
        }

        if first_cluster == 0 {
            first_cluster = cluster;
        }

        // Link previous cluster to this one
        if prev_cluster != 0 {
            *FAT_TABLE.add(prev_cluster as usize) = cluster;
        }

        // Prepare cluster data
        string::memset(cluster_buf, 0, BPB.cluster_size as usize);
        let mut write_size = BPB.cluster_size;
        if offset + write_size > size {
            write_size = size - offset;
        }
        string::memcpy(cluster_buf, buf.add(offset as usize), write_size as usize);

        if write_cluster(cluster as u32, cluster_buf) < 0 {
            heap::kfree(cluster_buf);
            return -1;
        }

        offset += write_size;
        prev_cluster = cluster;
    }

    heap::kfree(cluster_buf);

    // Update directory entry
    entry.first_cluster = first_cluster;
    entry.size = size;

    if write_root_entry(dir_index as u32, &entry) < 0 {
        return -1;
    }

    // Flush FAT to disk
    if fat_flush() < 0 {
        return -1;
    }

    0
}

// ---- Enumeration helpers for VFS adapter ------------------------------------

/// Get the number of regular files in the root directory.
pub unsafe fn get_file_count() -> i32 {
    let mut count: i32 = 0;
    let mut entry: Fat16DirEntry = core::mem::zeroed();

    if FAT16_READY == 0 {
        return 0;
    }

    for i in 0..BPB.root_entry_count as u32 {
        if read_root_entry(i, &mut entry) < 0 {
            break;
        }
        if entry.name[0] == 0x00 {
            break;
        }
        if entry.name[0] == 0xE5 {
            continue;
        }
        if entry.attr == 0x0F || (entry.attr & 0x08) != 0 {
            continue;
        }
        if (entry.attr & 0x10) != 0 {
            continue;
        }
        count += 1;
    }
    count
}

/// Get the filename at the given index (among regular files).
/// name_buf must be at least 13 bytes. Returns 0 on success, -1 on error.
pub unsafe fn get_file_name(index: i32, name_buf: *mut u8, buf_size: u32) -> i32 {
    let mut count: i32 = 0;
    let mut entry: Fat16DirEntry = core::mem::zeroed();

    if FAT16_READY == 0 || buf_size < 13 {
        return -1;
    }

    for i in 0..BPB.root_entry_count as u32 {
        if read_root_entry(i, &mut entry) < 0 {
            break;
        }
        if entry.name[0] == 0x00 {
            break;
        }
        if entry.name[0] == 0xE5 {
            continue;
        }
        if entry.attr == 0x0F || (entry.attr & 0x08) != 0 {
            continue;
        }
        if (entry.attr & 0x10) != 0 {
            continue;
        }

        if count == index {
            name_from_83(entry.name.as_ptr(), name_buf);
            return 0;
        }
        count += 1;
    }
    -1
}

/// Get the file size at the given index (among regular files).
pub unsafe fn get_file_size(index: i32) -> u32 {
    let mut count: i32 = 0;
    let mut entry: Fat16DirEntry = core::mem::zeroed();

    if FAT16_READY == 0 {
        return 0;
    }

    for i in 0..BPB.root_entry_count as u32 {
        if read_root_entry(i, &mut entry) < 0 {
            break;
        }
        if entry.name[0] == 0x00 {
            break;
        }
        if entry.name[0] == 0xE5 {
            continue;
        }
        if entry.attr == 0x0F || (entry.attr & 0x08) != 0 {
            continue;
        }
        if (entry.attr & 0x10) != 0 {
            continue;
        }

        if count == index {
            return entry.size;
        }
        count += 1;
    }
    0
}
