// fat16_vfs.rs -- Adapter that wraps FAT16 files as VFS nodes.
//
// Creates a /disk directory under vfs_root. Files on the FAT16 disk
// appear as children of /disk and can be read/written through the
// standard VFS interface (cat /disk/file.txt, etc.).
//
// The directory listing is dynamic -- readdir and finddir query the
// FAT16 driver each time, so newly written files appear immediately.

use crate::fat16;
use crate::heap;
use crate::string;
use crate::vfs::{self, VfsNode, VFS_FILE, VFS_DIRECTORY};

/// Maximum files we cache VFS nodes for.
const FAT16_VFS_MAX_FILES: usize = 64;

static mut DISK_DIR: *mut VfsNode = core::ptr::null_mut();

/// Cache of VFS file nodes (lazy-allocated).
static mut FILE_NODES: [*mut VfsNode; FAT16_VFS_MAX_FILES] =
    [core::ptr::null_mut(); FAT16_VFS_MAX_FILES];
static mut NODE_COUNT: i32 = 0;

// ---- VFS file callbacks -----------------------------------------------------

/// Read from a FAT16 file. The node's name holds the filename.
/// We read the entire file and then return the requested range.
fn fat16_vfs_read(node: *mut VfsNode, offset: u32, size: u32, buffer: *mut u8) -> i32 {
    unsafe {
        if node.is_null() {
            return -1;
        }

        if (*node).size == 0 {
            return 0;
        }

        if offset >= (*node).size {
            return 0;
        }

        // Read the whole file into a temp buffer
        let file_buf = heap::kmalloc((*node).size as usize);
        if file_buf.is_null() {
            return -1;
        }

        let bytes_read = fat16::read_file((*node).name.as_ptr(), file_buf, (*node).size);
        if bytes_read < 0 {
            heap::kfree(file_buf);
            return -1;
        }

        let avail = bytes_read as u32;
        if offset >= avail {
            heap::kfree(file_buf);
            return 0;
        }

        let mut sz = size;
        if sz > avail - offset {
            sz = avail - offset;
        }

        string::memcpy(buffer, file_buf.add(offset as usize), sz as usize);
        heap::kfree(file_buf);
        sz as i32
    }
}

/// Write to a FAT16 file. Writes the full file content from offset 0.
/// If offset > 0, we do a read-modify-write (read existing, overlay, write).
fn fat16_vfs_write(node: *mut VfsNode, offset: u32, size: u32, buffer: *const u8) -> i32 {
    unsafe {
        if node.is_null() {
            return -1;
        }

        // Simple case: writing from the beginning
        if offset == 0 {
            let ret = fat16::write_file((*node).name.as_ptr(), buffer, size);
            if ret < 0 {
                return -1;
            }
            (*node).size = size;
            return size as i32;
        }

        // Read-modify-write for non-zero offset
        let mut new_size = offset + size;
        if new_size < (*node).size {
            new_size = (*node).size;
        }

        let file_buf = heap::kmalloc(new_size as usize);
        if file_buf.is_null() {
            return -1;
        }

        string::memset(file_buf, 0, new_size as usize);

        // Read existing content
        if (*node).size > 0 {
            fat16::read_file((*node).name.as_ptr(), file_buf, (*node).size);
        }

        // Overlay the new data
        string::memcpy(file_buf.add(offset as usize), buffer, size as usize);

        let ret = fat16::write_file((*node).name.as_ptr(), file_buf, new_size);
        heap::kfree(file_buf);

        if ret < 0 {
            return -1;
        }

        (*node).size = new_size;
        size as i32
    }
}

// ---- VFS directory callbacks ------------------------------------------------

/// Refresh our cached node list from the FAT16 directory.
/// Called before readdir/finddir to pick up any changes.
unsafe fn refresh_nodes() {
    let mut count = fat16::get_file_count();
    if count > FAT16_VFS_MAX_FILES as i32 {
        count = FAT16_VFS_MAX_FILES as i32;
    }

    let mut i = 0;
    while i < count {
        let mut name = [0u8; 13];
        if fat16::get_file_name(i, name.as_mut_ptr(), 13) < 0 {
            break;
        }

        if i < NODE_COUNT && !FILE_NODES[i as usize].is_null() {
            // Update existing node
            let node = FILE_NODES[i as usize];
            string::strncpy((*node).name.as_mut_ptr(), name.as_ptr(), 63);
            (*node).name[63] = 0;
            (*node).size = fat16::get_file_size(i);
        } else {
            // Allocate new node
            let node = heap::kmalloc(core::mem::size_of::<VfsNode>()) as *mut VfsNode;
            if node.is_null() {
                break;
            }
            string::memset(node as *mut u8, 0, core::mem::size_of::<VfsNode>());

            string::strncpy((*node).name.as_mut_ptr(), name.as_ptr(), 63);
            (*node).name[63] = 0;
            (*node).flags = VFS_FILE;
            (*node).size = fat16::get_file_size(i);
            (*node).inode = i as u32;
            (*node).read_fn = Some(fat16_vfs_read);
            (*node).write_fn = Some(fat16_vfs_write);

            FILE_NODES[i as usize] = node;
        }
        i += 1;
    }

    NODE_COUNT = i;
}

fn fat16_vfs_readdir(_dir: *mut VfsNode, index: u32) -> *mut VfsNode {
    unsafe {
        refresh_nodes();
        if index >= NODE_COUNT as u32 {
            return core::ptr::null_mut();
        }
        FILE_NODES[index as usize]
    }
}

fn fat16_vfs_finddir(_dir: *mut VfsNode, name: *const u8) -> *mut VfsNode {
    unsafe {
        refresh_nodes();

        for i in 0..NODE_COUNT {
            if string::strcmp((*FILE_NODES[i as usize]).name.as_ptr(), name) == 0 {
                return FILE_NODES[i as usize];
            }
        }
        core::ptr::null_mut()
    }
}

// ---- Initialization ---------------------------------------------------------

/// Register FAT16 files into VFS root under /disk.
pub unsafe fn init() {
    // Create /disk directory node
    DISK_DIR = heap::kmalloc(core::mem::size_of::<VfsNode>()) as *mut VfsNode;
    if DISK_DIR.is_null() {
        return;
    }
    string::memset(DISK_DIR as *mut u8, 0, core::mem::size_of::<VfsNode>());

    string::memcpy((*DISK_DIR).name.as_mut_ptr(), b"disk".as_ptr(), 4);
    (*DISK_DIR).name[4] = 0;

    (*DISK_DIR).flags = VFS_DIRECTORY;
    (*DISK_DIR).readdir_fn = Some(fat16_vfs_readdir);
    (*DISK_DIR).finddir_fn = Some(fat16_vfs_finddir);
    (*DISK_DIR).num_children = 0;

    // Initial population
    refresh_nodes();

    // Attach /disk to root
    vfs::add_child(vfs::VFS_ROOT, DISK_DIR);
}
