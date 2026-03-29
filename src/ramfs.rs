// ramfs.rs -- RAM-backed writable filesystem.
//
// Each file is backed by a kmalloc'd buffer that grows on demand.
// Files live under vfs_root and are lost on reboot.

use crate::heap;
use crate::string;
use crate::vfs::{self, VfsNode, VFS_FILE};

#[repr(C)]
struct RamfsFileData {
    data: *mut u8,
    size: u32,
    capacity: u32,
}

static mut RAMFS_ROOT: *mut VfsNode = core::ptr::null_mut();

// ---- File operations ---------------------------------------------------------

fn ramfs_read(node: *mut VfsNode, offset: u32, size: u32, buffer: *mut u8) -> i32 {
    unsafe {
        if node.is_null() || (*node).private_data.is_null() {
            return -1;
        }

        let fd = (*node).private_data as *mut RamfsFileData;

        if offset >= (*fd).size {
            return 0;
        }
        let mut sz = size;
        // Guard against integer overflow in offset + size
        if sz > 0xFFFFFFFF - offset {
            sz = (*fd).size - offset;
        }
        if offset + sz > (*fd).size {
            sz = (*fd).size - offset;
        }

        string::memcpy(buffer, (*fd).data.add(offset as usize), sz as usize);
        sz as i32
    }
}

fn ramfs_write(node: *mut VfsNode, offset: u32, size: u32, buffer: *const u8) -> i32 {
    unsafe {
        if node.is_null() || (*node).private_data.is_null() {
            return -1;
        }

        let fd = (*node).private_data as *mut RamfsFileData;

        // Guard against integer overflow in offset + size
        if size > 0xFFFFFFFF - offset {
            return -1;
        }

        let needed = offset + size;

        // Grow buffer if necessary
        if needed > (*fd).capacity {
            let mut new_cap = (*fd).capacity;
            if new_cap < 256 {
                new_cap = 256;
            }
            while new_cap < needed {
                new_cap *= 2;
            }

            let new_buf = heap::kmalloc(new_cap as usize) as *mut u8;
            if new_buf.is_null() {
                return -1;
            }
            string::memset(new_buf, 0, new_cap as usize);

            if !(*fd).data.is_null() {
                string::memcpy(new_buf, (*fd).data, (*fd).size as usize);
                heap::kfree((*fd).data as *mut u8);
            }
            (*fd).data = new_buf;
            (*fd).capacity = new_cap;
        }

        string::memcpy((*fd).data.add(offset as usize), buffer, size as usize);

        // Update logical size: max of current size and write end position
        if needed > (*fd).size {
            (*fd).size = needed;
        }
        (*node).size = (*fd).size;

        size as i32
    }
}

// ---- Public API --------------------------------------------------------------

/// Create a new writable ramfs file under the VFS root.
/// Returns the existing node if a file with this name already exists.
pub unsafe fn create_file(name: *const u8) -> *mut VfsNode {
    if RAMFS_ROOT.is_null() || name.is_null() {
        return core::ptr::null_mut();
    }

    // Return existing file if one with this name already exists
    let existing = vfs::finddir(RAMFS_ROOT, name);
    if !existing.is_null() {
        return existing;
    }

    // Allocate and zero the VFS node
    let node = heap::kmalloc(core::mem::size_of::<VfsNode>()) as *mut VfsNode;
    if node.is_null() {
        return core::ptr::null_mut();
    }
    string::memset(node as *mut u8, 0, core::mem::size_of::<VfsNode>());

    // Copy name (max 63 chars + null)
    let len = string::strlen(name);
    let copy_len = if len > 63 { 63 } else { len };
    string::memcpy((*node).name.as_mut_ptr(), name, copy_len);
    (*node).name[copy_len] = 0;

    (*node).flags = VFS_FILE;
    (*node).read_fn = Some(ramfs_read);
    (*node).write_fn = Some(ramfs_write);

    // Allocate backing data with initial capacity
    let fd = heap::kmalloc(core::mem::size_of::<RamfsFileData>()) as *mut RamfsFileData;
    if fd.is_null() {
        heap::kfree(node as *mut u8);
        return core::ptr::null_mut();
    }
    let data_buf = heap::kmalloc(256) as *mut u8;
    if data_buf.is_null() {
        heap::kfree(fd as *mut u8);
        heap::kfree(node as *mut u8);
        return core::ptr::null_mut();
    }
    string::memset(data_buf, 0, 256);
    (*fd).data = data_buf;
    (*fd).size = 0;
    (*fd).capacity = 256;

    (*node).private_data = fd as *mut u8;

    // Add to VFS root
    if vfs::add_child(RAMFS_ROOT, node) < 0 {
        heap::kfree((*fd).data);
        heap::kfree(fd as *mut u8);
        heap::kfree(node as *mut u8);
        return core::ptr::null_mut();
    }
    node
}

/// Initialize ramfs by recording the VFS root pointer.
pub unsafe fn init() {
    RAMFS_ROOT = vfs::VFS_ROOT;
}
