// ata.rs -- ATA PIO disk driver.
//
// Communicates with the primary ATA controller via I/O ports 0x1F0-0x1F7.
// Uses 28-bit LBA addressing and polling (no IRQ, no DMA).
//
// Port map (primary channel):
//   0x1F0  Data register (16-bit read/write)
//   0x1F1  Error (read) / Features (write)
//   0x1F2  Sector Count
//   0x1F3  LBA Low  (bits 0-7)
//   0x1F4  LBA Mid  (bits 8-15)
//   0x1F5  LBA High (bits 16-23)
//   0x1F6  Device/Head (bits 24-27 of LBA, bit 6=LBA, bit 4=drive)
//   0x1F7  Status (read) / Command (write)

use crate::io;
use crate::vga;

pub const ATA_SECTOR_SIZE: u32 = 512;

// I/O ports
const ATA_DATA: u16 = 0x1F0;
#[allow(dead_code)]
const ATA_ERROR: u16 = 0x1F1;
const ATA_SECT_COUNT: u16 = 0x1F2;
const ATA_LBA_LOW: u16 = 0x1F3;
const ATA_LBA_MID: u16 = 0x1F4;
const ATA_LBA_HIGH: u16 = 0x1F5;
const ATA_DRIVE_HEAD: u16 = 0x1F6;
const ATA_STATUS: u16 = 0x1F7;
const ATA_COMMAND: u16 = 0x1F7;

// Status register bits
const ATA_SR_BSY: u8 = 0x80;  // busy
const ATA_SR_DRDY: u8 = 0x40; // drive ready
const ATA_SR_DRQ: u8 = 0x08;  // data request
const ATA_SR_ERR: u8 = 0x01;  // error

// Commands
const ATA_CMD_READ: u8 = 0x20;     // READ SECTORS
const ATA_CMD_WRITE: u8 = 0x30;    // WRITE SECTORS
const ATA_CMD_IDENTIFY: u8 = 0xEC; // IDENTIFY DEVICE
const ATA_CMD_FLUSH: u8 = 0xE7;    // CACHE FLUSH

// Timeout: number of status reads before giving up
const ATA_TIMEOUT: i32 = 100000;

static mut ATA_PRESENT: i32 = 0; // 1 if a drive was detected

/// Wait until BSY clears. Returns 0 on success, -1 on timeout.
unsafe fn ata_wait_bsy() -> i32 {
    for _ in 0..ATA_TIMEOUT {
        if (io::inb(ATA_STATUS) & ATA_SR_BSY) == 0 {
            return 0;
        }
    }
    -1
}

/// Wait until BSY clears and DRQ sets. Returns 0 on success, -1 on error.
unsafe fn ata_wait_drq() -> i32 {
    for _ in 0..ATA_TIMEOUT {
        let status = io::inb(ATA_STATUS);
        if (status & ATA_SR_ERR) != 0 {
            return -1;
        }
        if (status & ATA_SR_BSY) == 0 && (status & ATA_SR_DRQ) != 0 {
            return 0;
        }
    }
    -1
}

/// Wait until BSY clears and DRDY sets. Returns 0 on success, -1 on error.
unsafe fn ata_wait_ready() -> i32 {
    for _ in 0..ATA_TIMEOUT {
        let status = io::inb(ATA_STATUS);
        if (status & ATA_SR_ERR) != 0 {
            return -1;
        }
        if (status & ATA_SR_BSY) == 0 && (status & ATA_SR_DRDY) != 0 {
            return 0;
        }
    }
    -1
}

/// Select drive 0, set LBA address and sector count.
unsafe fn ata_select_sector(lba: u32) {
    // Drive 0, LBA mode (bit 6), bits 24-27 of LBA in low nibble
    io::outb(ATA_DRIVE_HEAD, 0xE0 | ((lba >> 24) & 0x0F) as u8);
    io::outb(ATA_SECT_COUNT, 1); // one sector
    io::outb(ATA_LBA_LOW, (lba & 0xFF) as u8);
    io::outb(ATA_LBA_MID, ((lba >> 8) & 0xFF) as u8);
    io::outb(ATA_LBA_HIGH, ((lba >> 16) & 0xFF) as u8);
}

/// Detect whether an ATA drive is present on the primary channel.
/// Sends IDENTIFY DEVICE and checks for a valid response.
/// Returns 0 on success, -1 on failure.
pub unsafe fn init() -> i32 {
    ATA_PRESENT = 0;

    // Select drive 0
    io::outb(ATA_DRIVE_HEAD, 0xE0);

    // Zero the sector count and LBA registers
    io::outb(ATA_SECT_COUNT, 0);
    io::outb(ATA_LBA_LOW, 0);
    io::outb(ATA_LBA_MID, 0);
    io::outb(ATA_LBA_HIGH, 0);

    // Send IDENTIFY command
    io::outb(ATA_COMMAND, ATA_CMD_IDENTIFY);

    // Read status -- if 0, no drive
    let status = io::inb(ATA_STATUS);
    if status == 0 {
        vga::puts(b"ATA: no drive detected\n");
        return -1;
    }

    // Wait for BSY to clear
    if ata_wait_bsy() < 0 {
        vga::puts(b"ATA: timeout waiting for drive\n");
        return -1;
    }

    // Check that LBA mid and high are still 0 (not ATAPI/SATA)
    if io::inb(ATA_LBA_MID) != 0 || io::inb(ATA_LBA_HIGH) != 0 {
        vga::puts(b"ATA: not an ATA drive\n");
        return -1;
    }

    // Wait for DRQ or ERR
    if ata_wait_drq() < 0 {
        vga::puts(b"ATA: IDENTIFY failed\n");
        return -1;
    }

    // Read and discard 256 words of identification data
    for _ in 0..256 {
        let _ = io::inw(ATA_DATA);
    }

    ATA_PRESENT = 1;
    vga::puts(b"ATA: drive detected\n");
    0
}

/// Read one 512-byte sector at the given LBA into buf.
pub unsafe fn read_sector(lba: u32, buf: *mut u8) -> i32 {
    let p = buf as *mut u16;

    if ATA_PRESENT == 0 {
        return -1;
    }

    if ata_wait_ready() < 0 {
        return -1;
    }

    ata_select_sector(lba);
    io::outb(ATA_COMMAND, ATA_CMD_READ);

    if ata_wait_drq() < 0 {
        return -1;
    }

    for i in 0..256 {
        *p.add(i) = io::inw(ATA_DATA);
    }

    0
}

/// Write one 512-byte sector from buf to the given LBA.
pub unsafe fn write_sector(lba: u32, buf: *const u8) -> i32 {
    let p = buf as *const u16;

    if ATA_PRESENT == 0 {
        return -1;
    }

    if ata_wait_ready() < 0 {
        return -1;
    }

    ata_select_sector(lba);
    io::outb(ATA_COMMAND, ATA_CMD_WRITE);

    if ata_wait_drq() < 0 {
        return -1;
    }

    for i in 0..256 {
        io::outw(ATA_DATA, *p.add(i));
    }

    // Flush the write cache
    io::outb(ATA_COMMAND, ATA_CMD_FLUSH);
    if ata_wait_bsy() < 0 {
        return -1;
    }

    0
}

/// Read multiple sectors into buf. Returns 0 on success, -1 on failure.
pub unsafe fn read_sectors(lba: u32, count: u8, buf: *mut u8) -> i32 {
    for i in 0..count as u32 {
        if read_sector(lba + i, buf.add((i * 512) as usize)) < 0 {
            return -1;
        }
    }
    0
}

/// Write multiple sectors from buf. Returns 0 on success, -1 on failure.
pub unsafe fn write_sectors(lba: u32, count: u8, buf: *const u8) -> i32 {
    for i in 0..count as u32 {
        if write_sector(lba + i, buf.add((i * 512) as usize)) < 0 {
            return -1;
        }
    }
    0
}
