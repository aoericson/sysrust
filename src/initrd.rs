// initrd.rs -- Initial ramdisk parser + VFS adapter.
//
// Parses the flat initrd image loaded as a Multiboot module by the
// bootloader. The image format is:
//   [4 bytes: file count]
//   For each file:
//     [64 bytes: null-padded filename]
//     [4 bytes: file size]
//     [N bytes: file data]
//
// Also provides init_vfs() which wraps each initrd file as a read-only
// VFS node and attaches them to the VFS root.

use crate::heap;
use crate::multiboot::{MultibootInfo, ModEntry, MULTIBOOT_FLAG_MODS};
use crate::string;
use crate::vga;
use crate::vfs::{self, VfsNode, VFS_FILE};

const INITRD_MAX_FILES: usize = 64;
const INITRD_NAME_LEN: usize = 64;

#[repr(C)]
pub struct InitrdFile {
    pub name: [u8; INITRD_NAME_LEN],
    pub size: u32,
    pub data: *const u8,
}

static mut FILES: [InitrdFile; INITRD_MAX_FILES] = {
    const EMPTY: InitrdFile = InitrdFile {
        name: [0u8; INITRD_NAME_LEN],
        size: 0,
        data: core::ptr::null(),
    };
    [EMPTY; INITRD_MAX_FILES]
};

static mut FILE_COUNT: u32 = 0;

// ---- Public API --------------------------------------------------------------

pub unsafe fn init(mb: &MultibootInfo) {
    FILE_COUNT = 0;

    // Check if modules were loaded
    if mb.flags & MULTIBOOT_FLAG_MODS == 0 || mb.mods_count == 0 {
        vga::puts(b"initrd: no modules loaded\n");
        return;
    }

    // Use the first module as the initrd
    let mod_entry = &*(mb.mods_addr as *const ModEntry);
    let mut ptr = mod_entry.mod_start as *const u8;

    // Read file count
    let mut count: u32 = 0;
    string::memcpy(
        &mut count as *mut u32 as *mut u8,
        ptr,
        4,
    );
    ptr = ptr.add(4);

    if count > INITRD_MAX_FILES as u32 {
        count = INITRD_MAX_FILES as u32;
    }

    for i in 0..count as usize {
        string::memcpy(FILES[i].name.as_mut_ptr(), ptr, INITRD_NAME_LEN);
        FILES[i].name[INITRD_NAME_LEN - 1] = 0;
        ptr = ptr.add(INITRD_NAME_LEN);

        string::memcpy(
            &mut FILES[i].size as *mut u32 as *mut u8,
            ptr,
            4,
        );
        ptr = ptr.add(4);

        FILES[i].data = ptr;
        ptr = ptr.add(FILES[i].size as usize);
    }

    FILE_COUNT = count;

    vga::puts(b"initrd: ");
    // Print count
    if count == 0 {
        vga::putchar(b'0');
    } else {
        let mut buf = [0u8; 12];
        let mut pos = 0usize;
        let mut v = count;
        let mut tmp = [0u8; 12];
        let mut len = 0usize;
        while v > 0 {
            tmp[len] = b'0' + (v % 10) as u8;
            v /= 10;
            len += 1;
        }
        while len > 0 {
            len -= 1;
            buf[pos] = tmp[len];
            pos += 1;
        }
        buf[pos] = 0;
        vga::puts(&buf[..pos]);
    }
    vga::puts(b" files\n");
}

pub unsafe fn get_count() -> u32 {
    FILE_COUNT
}

pub unsafe fn get_file(index: u32) -> Option<&'static InitrdFile> {
    if index >= FILE_COUNT {
        return None;
    }
    Some(&FILES[index as usize])
}

pub unsafe fn find(name: *const u8) -> Option<&'static InitrdFile> {
    for i in 0..FILE_COUNT as usize {
        if string::strcmp(FILES[i].name.as_ptr(), name) == 0 {
            return Some(&FILES[i]);
        }
    }
    None
}

// ---- VFS adapter (ported from initrd_vfs.c) ----------------------------------

fn initrd_vfs_read(node: *mut VfsNode, offset: u32, size: u32, buffer: *mut u8) -> i32 {
    unsafe {
        let f = (*node).private_data as *const InitrdFile;
        if f.is_null() {
            return -1;
        }
        if offset >= (*f).size {
            return 0;
        }
        let mut sz = size;
        // Guard against integer overflow in offset + size
        if sz > 0xFFFFFFFF - offset {
            sz = (*f).size - offset;
        }
        if offset + sz > (*f).size {
            sz = (*f).size - offset;
        }
        string::memcpy(buffer, (*f).data.add(offset as usize), sz as usize);
        sz as i32
    }
}

/// Mount all initrd files into the VFS as children of `root`.
pub unsafe fn init_vfs(root: *mut VfsNode) {
    let count = get_count();

    for i in 0..count {
        let f = match get_file(i) {
            Some(f) => f,
            None => continue,
        };

        let node = heap::kmalloc(core::mem::size_of::<VfsNode>()) as *mut VfsNode;
        if node.is_null() {
            continue;
        }
        string::memset(node as *mut u8, 0, core::mem::size_of::<VfsNode>());

        let len = string::strlen(f.name.as_ptr());
        let copy_len = if len > 63 { 63 } else { len };
        string::memcpy((*node).name.as_mut_ptr(), f.name.as_ptr(), copy_len);
        (*node).name[copy_len] = 0;

        (*node).flags = VFS_FILE;
        (*node).size = f.size;
        (*node).inode = i;
        (*node).read_fn = Some(initrd_vfs_read);
        (*node).write_fn = None;
        (*node).readdir_fn = None;
        (*node).finddir_fn = None;
        (*node).private_data = f as *const InitrdFile as *mut u8;

        vfs::add_child(root, node);
    }
}
