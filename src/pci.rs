// pci.rs -- PCI bus configuration space interface.
//
// PCI configuration space is accessed via two I/O ports:
//   0xCF8 (CONFIG_ADDRESS) -- write a 32-bit address identifying the register
//   0xCFC (CONFIG_DATA)    -- read/write the 32-bit register value
//
// The address format is:
//   bit 31    = enable (must be 1)
//   bits 23-16 = bus number (0-255)
//   bits 15-11 = device number (0-31)
//   bits 10-8  = function number (0-7)
//   bits 7-2   = register offset (aligned to 4 bytes)
//   bits 1-0   = must be 0

use crate::io::{inl, outl};

const PCI_CONFIG_ADDR: u16 = 0xCF8;
const PCI_CONFIG_DATA: u16 = 0xCFC;

/// Describes a PCI device found during bus scan.
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub bar0: u32,
    pub irq_line: u8,
}

/// Build the 32-bit address for a PCI config register access.
fn pci_address(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    (1u32 << 31)
        | ((bus as u32) << 16)
        | (((device & 0x1F) as u32) << 11)
        | (((function & 0x07) as u32) << 8)
        | ((offset as u32) & 0xFC)
}

/// Read a 32-bit value from PCI configuration space.
pub fn config_read32(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    unsafe {
        outl(PCI_CONFIG_ADDR, pci_address(bus, device, function, offset));
        inl(PCI_CONFIG_DATA)
    }
}

/// Read a 16-bit value from PCI configuration space.
pub fn config_read16(bus: u8, device: u8, function: u8, offset: u8) -> u16 {
    unsafe {
        outl(PCI_CONFIG_ADDR, pci_address(bus, device, function, offset));
        let val = inl(PCI_CONFIG_DATA);
        ((val >> (((offset & 2) as u32) * 8)) & 0xFFFF) as u16
    }
}

/// Write a 16-bit value to PCI configuration space.
pub fn config_write16(bus: u8, device: u8, function: u8, offset: u8, value: u16) {
    unsafe {
        let addr = pci_address(bus, device, function, offset);
        outl(PCI_CONFIG_ADDR, addr);
        let mut old = inl(PCI_CONFIG_DATA);

        let shift = ((offset & 2) as u32) * 8;
        old &= !(0xFFFFu32 << shift);
        old |= (value as u32) << shift;

        outl(PCI_CONFIG_ADDR, addr);
        outl(PCI_CONFIG_DATA, old);
    }
}

/// Write a 32-bit value to PCI configuration space.
pub fn config_write32(bus: u8, device: u8, function: u8, offset: u8, value: u32) {
    unsafe {
        outl(PCI_CONFIG_ADDR, pci_address(bus, device, function, offset));
        outl(PCI_CONFIG_DATA, value);
    }
}

/// Scan all PCI buses for a device matching the given vendor/device ID.
/// Returns `Some(PciDevice)` if found, `None` otherwise.
pub fn find_device(vendor_id: u16, device_id: u16) -> Option<PciDevice> {
    for bus in 0u16..256 {
        for device_num in 0u8..32 {
            for function in 0u8..8 {
                let reg0 = config_read32(bus as u8, device_num, function, 0x00);

                // Vendor ID is the low 16 bits, device ID is the high 16 bits
                if (reg0 & 0xFFFF) == 0xFFFF {
                    continue; // no device here
                }

                if (reg0 & 0xFFFF) == vendor_id as u32
                    && ((reg0 >> 16) & 0xFFFF) == device_id as u32
                {
                    // Read BAR0 (offset 0x10)
                    let bar0_raw = config_read32(bus as u8, device_num, function, 0x10);
                    let bar0 = if bar0_raw & 1 != 0 {
                        bar0_raw & 0xFFFFFFFC // I/O space: mask low 2 bits
                    } else {
                        bar0_raw & 0xFFFFFFF0 // memory space: mask low 4 bits
                    };

                    // Read interrupt line (offset 0x3C, low byte)
                    let reg3c = config_read32(bus as u8, device_num, function, 0x3C);
                    let irq_line = (reg3c & 0xFF) as u8;

                    return Some(PciDevice {
                        bus: bus as u8,
                        device: device_num,
                        function,
                        vendor_id,
                        device_id,
                        bar0,
                        irq_line,
                    });
                }
            }
        }
    }
    None
}

/// Enable PCI bus mastering for a device.
/// Sets bit 2 of the PCI Command Register (offset 0x04).
/// Required for any device that uses DMA (like the RTL8139).
pub fn enable_bus_mastering(dev: &PciDevice) {
    let mut cmd = config_read16(dev.bus, dev.device, dev.function, 0x04);
    cmd |= 1 << 2; // set bus master bit
    config_write16(dev.bus, dev.device, dev.function, 0x04, cmd);
}
