// vfs.rs -- Virtual File System core.
//
// Manages the root directory, dispatches operations through node function
// pointers, and maintains a global file descriptor table.
//
// Supports hierarchical directory trees with path resolution.

use crate::heap;
use crate::string;
use crate::vga;

// Node type flags
pub const VFS_FILE: u32 = 0x01;
pub const VFS_DIRECTORY: u32 = 0x02;
pub const VFS_DEVICE: u32 = 0x04;

pub const VFS_MAX_CHILDREN: usize = 128;
const VFS_NAME_LEN: usize = 64;
const MAX_OPEN_FILES: usize = 32;

// Function pointer types for VFS operations
pub type ReadFn = fn(*mut VfsNode, u32, u32, *mut u8) -> i32;
pub type WriteFn = fn(*mut VfsNode, u32, u32, *const u8) -> i32;
pub type ReaddirFn = fn(*mut VfsNode, u32) -> *mut VfsNode;
pub type FinddirFn = fn(*mut VfsNode, *const u8) -> *mut VfsNode;

#[repr(C)]
pub struct VfsNode {
    pub name: [u8; VFS_NAME_LEN],
    pub flags: u32,
    pub size: u32,
    pub inode: u32,
    pub read_fn: Option<ReadFn>,
    pub write_fn: Option<WriteFn>,
    pub readdir_fn: Option<ReaddirFn>,
    pub finddir_fn: Option<FinddirFn>,
    pub private_data: *mut u8,
    pub children: [*mut VfsNode; VFS_MAX_CHILDREN],
    pub num_children: u32,
    pub parent: *mut VfsNode,
}

// Global root directory
pub static mut VFS_ROOT: *mut VfsNode = core::ptr::null_mut();

// File descriptor table
pub struct FdEntry {
    pub node: *mut VfsNode,
    pub offset: u32,
    pub in_use: bool,
}

pub static mut FD_TABLE: [FdEntry; MAX_OPEN_FILES] = {
    const EMPTY: FdEntry = FdEntry {
        node: core::ptr::null_mut(),
        offset: 0,
        in_use: false,
    };
    [EMPTY; MAX_OPEN_FILES]
};

pub const FD_TABLE_SIZE: usize = MAX_OPEN_FILES;

// ---- Default directory operations -------------------------------------------

fn default_readdir(dir: *mut VfsNode, index: u32) -> *mut VfsNode {
    unsafe {
        if (*dir).flags & VFS_DIRECTORY == 0 {
            return core::ptr::null_mut();
        }
        if index >= (*dir).num_children {
            return core::ptr::null_mut();
        }
        (*dir).children[index as usize]
    }
}

fn default_finddir(dir: *mut VfsNode, name: *const u8) -> *mut VfsNode {
    unsafe {
        if (*dir).flags & VFS_DIRECTORY == 0 {
            return core::ptr::null_mut();
        }
        for i in 0..(*dir).num_children as usize {
            if string::strcmp((*(*dir).children[i]).name.as_ptr(), name) == 0 {
                return (*dir).children[i];
            }
        }
        core::ptr::null_mut()
    }
}

// ---- Public dispatch functions -----------------------------------------------

pub unsafe fn read(node: *mut VfsNode, offset: u32, size: u32, buffer: *mut u8) -> i32 {
    if node.is_null() {
        return -1;
    }
    match (*node).read_fn {
        Some(f) => f(node, offset, size, buffer),
        None => -1,
    }
}

pub unsafe fn write(node: *mut VfsNode, offset: u32, size: u32, buffer: *const u8) -> i32 {
    if node.is_null() {
        return -1;
    }
    match (*node).write_fn {
        Some(f) => f(node, offset, size, buffer),
        None => -1,
    }
}

pub unsafe fn readdir(dir: *mut VfsNode, index: u32) -> *mut VfsNode {
    if dir.is_null() {
        return core::ptr::null_mut();
    }
    match (*dir).readdir_fn {
        Some(f) => f(dir, index),
        None => core::ptr::null_mut(),
    }
}

pub unsafe fn finddir(dir: *mut VfsNode, name: *const u8) -> *mut VfsNode {
    if dir.is_null() {
        return core::ptr::null_mut();
    }
    match (*dir).finddir_fn {
        Some(f) => f(dir, name),
        None => core::ptr::null_mut(),
    }
}

pub unsafe fn add_child(parent: *mut VfsNode, child: *mut VfsNode) -> i32 {
    if parent.is_null() || child.is_null() {
        return -1;
    }
    if (*parent).flags & VFS_DIRECTORY == 0 {
        return -1;
    }
    if (*parent).num_children >= VFS_MAX_CHILDREN as u32 {
        return -1;
    }
    (*parent).children[(*parent).num_children as usize] = child;
    (*parent).num_children += 1;
    (*child).parent = parent;
    0
}

// ---- mkdir: create a subdirectory node under parent -------------------------

/// Create a subdirectory named `name` under `parent`.
/// Returns the new directory node, or null on failure.
pub unsafe fn mkdir(parent: *mut VfsNode, name: *const u8) -> *mut VfsNode {
    if parent.is_null() || name.is_null() {
        return core::ptr::null_mut();
    }
    if (*parent).flags & VFS_DIRECTORY == 0 {
        return core::ptr::null_mut();
    }

    // If a child with this name already exists and is a directory, return it
    let existing = finddir(parent, name);
    if !existing.is_null() {
        if (*existing).flags & VFS_DIRECTORY != 0 {
            return existing;
        }
        // Name collision with a non-directory
        return core::ptr::null_mut();
    }

    let dir = make_dir(name);
    if dir.is_null() {
        return core::ptr::null_mut();
    }

    if add_child(parent, dir) < 0 {
        heap::kfree(dir as *mut u8);
        return core::ptr::null_mut();
    }

    dir
}

// ---- Path resolution ---------------------------------------------------------

/// Walk a `/`-separated path from VFS_ROOT and return the final node.
/// Returns null if any component along the path is not found.
pub unsafe fn resolve_path(path: *const u8) -> *mut VfsNode {
    if path.is_null() {
        return core::ptr::null_mut();
    }

    let mut cur = VFS_ROOT;
    let mut p = path;

    // Skip leading slashes
    while *p == b'/' {
        p = p.add(1);
    }

    // Empty path => root
    if *p == 0 {
        return cur;
    }

    while *p != 0 && !cur.is_null() {
        let mut component = [0u8; VFS_NAME_LEN];
        let mut ci = 0usize;

        while *p != 0 && *p != b'/' && ci < 63 {
            component[ci] = *p;
            ci += 1;
            p = p.add(1);
        }
        component[ci] = 0;

        // If name was truncated, advance past the rest of this component
        while *p != 0 && *p != b'/' {
            p = p.add(1);
        }

        // Skip trailing slashes
        while *p == b'/' {
            p = p.add(1);
        }

        cur = finddir(cur, component.as_ptr());
    }

    cur
}

// ---- mkdir_p: create path recursively (like mkdir -p) -----------------------

/// Walk `path` from VFS_ROOT, creating intermediate directories as needed.
/// Returns the final directory node, or null on failure.
pub unsafe fn mkdir_p(path: *const u8) -> *mut VfsNode {
    if path.is_null() {
        return core::ptr::null_mut();
    }

    let mut cur = VFS_ROOT;
    let mut p = path;

    // Skip leading slashes
    while *p == b'/' {
        p = p.add(1);
    }

    // Empty path => root
    if *p == 0 {
        return cur;
    }

    while *p != 0 && !cur.is_null() {
        let mut component = [0u8; VFS_NAME_LEN];
        let mut ci = 0usize;

        while *p != 0 && *p != b'/' && ci < 63 {
            component[ci] = *p;
            ci += 1;
            p = p.add(1);
        }
        component[ci] = 0;

        // Skip past truncated remainder
        while *p != 0 && *p != b'/' {
            p = p.add(1);
        }

        // Skip trailing slashes
        while *p == b'/' {
            p = p.add(1);
        }

        // Try to find the component; if not found, create it
        let child = finddir(cur, component.as_ptr());
        if child.is_null() {
            // Create the intermediate directory
            cur = mkdir(cur, component.as_ptr());
        } else {
            cur = child;
        }
    }

    cur
}

// ---- unlink: remove a child node from its parent ----------------------------

/// Remove the child named `name` from `parent`.
/// Frees ramfs backing data if present.
/// Returns 0 on success, -1 on failure.
pub unsafe fn unlink(parent: *mut VfsNode, name: *const u8) -> i32 {
    if parent.is_null() || name.is_null() {
        return -1;
    }
    if (*parent).flags & VFS_DIRECTORY == 0 {
        return -1;
    }

    let count = (*parent).num_children as usize;
    let mut found_idx: Option<usize> = None;

    for i in 0..count {
        let child = (*parent).children[i];
        if !child.is_null() && string::strcmp((*child).name.as_ptr(), name) == 0 {
            found_idx = Some(i);
            break;
        }
    }

    match found_idx {
        None => -1,
        Some(idx) => {
            let child = (*parent).children[idx];

            // Free ramfs backing data if present
            if !(*child).private_data.is_null() {
                // The private_data points to a RamfsFileData struct
                // whose first field is a data pointer
                let data_ptr = *((*child).private_data as *const *mut u8);
                if !data_ptr.is_null() {
                    heap::kfree(data_ptr);
                }
                heap::kfree((*child).private_data);
            }

            // Free the node itself
            heap::kfree(child as *mut u8);

            // Shift remaining children down
            let last = count - 1;
            for i in idx..last {
                (*parent).children[i] = (*parent).children[i + 1];
            }
            (*parent).children[last] = core::ptr::null_mut();
            (*parent).num_children -= 1;

            0
        }
    }
}

/// Unlink a node by full path. Resolves the parent directory, then removes the child.
/// Returns 0 on success, -1 on failure.
pub unsafe fn unlink_path(path: *const u8) -> i32 {
    if path.is_null() {
        return -1;
    }

    // Find the last '/' to split into parent path + child name
    let len = string::strlen(path);
    if len == 0 {
        return -1;
    }

    // Find index of last '/' in the path
    let mut last_slash: i32 = -1;
    for i in 0..len {
        if *path.add(i) == b'/' {
            last_slash = i as i32;
        }
    }

    let parent: *mut VfsNode;
    let name: *const u8;

    if last_slash < 0 {
        // No slash -> file is in root
        parent = VFS_ROOT;
        name = path;
    } else if last_slash == 0 {
        // File directly under root, e.g. "/foo"
        parent = VFS_ROOT;
        name = path.add(1);
    } else {
        // Build parent path
        let plen = last_slash as usize;
        let mut parent_buf = [0u8; 256];
        if plen >= 255 {
            return -1;
        }
        string::memcpy(parent_buf.as_mut_ptr(), path, plen);
        parent_buf[plen] = 0;

        parent = resolve_path(parent_buf.as_ptr());
        if parent.is_null() {
            return -1;
        }
        name = path.add(last_slash as usize + 1);
    }

    if *name == 0 {
        return -1;
    }

    unlink(parent, name)
}

// ---- File descriptor operations ----------------------------------------------

pub unsafe fn vfs_open(path: *const u8) -> i32 {
    let node = resolve_path(path);
    if node.is_null() {
        return -1;
    }

    for i in 0..MAX_OPEN_FILES {
        if !FD_TABLE[i].in_use {
            FD_TABLE[i].node = node;
            FD_TABLE[i].offset = 0;
            FD_TABLE[i].in_use = true;
            return i as i32;
        }
    }

    -1 // no free fd
}

pub unsafe fn vfs_fd_read(fd: i32, buf: *mut u8, size: u32) -> i32 {
    if fd < 0 || fd >= MAX_OPEN_FILES as i32 || !FD_TABLE[fd as usize].in_use {
        return -1;
    }

    let bytes = read(FD_TABLE[fd as usize].node, FD_TABLE[fd as usize].offset, size, buf);
    if bytes > 0 {
        FD_TABLE[fd as usize].offset += bytes as u32;
    }
    bytes
}

pub unsafe fn vfs_fd_write(fd: i32, buf: *const u8, size: u32) -> i32 {
    if fd < 0 || fd >= MAX_OPEN_FILES as i32 || !FD_TABLE[fd as usize].in_use {
        return -1;
    }

    let bytes = write(
        FD_TABLE[fd as usize].node,
        FD_TABLE[fd as usize].offset,
        size,
        buf,
    );
    if bytes > 0 {
        FD_TABLE[fd as usize].offset += bytes as u32;
    }
    bytes
}

pub unsafe fn vfs_close(fd: i32) {
    if fd < 0 || fd >= MAX_OPEN_FILES as i32 {
        return;
    }
    FD_TABLE[fd as usize].node = core::ptr::null_mut();
    FD_TABLE[fd as usize].offset = 0;
    FD_TABLE[fd as usize].in_use = false;
}

/// Seek within an open file descriptor.
/// whence: 0=SEEK_SET, 1=SEEK_CUR, 2=SEEK_END
/// Returns the new offset, or -1 on error.
pub unsafe fn vfs_lseek(fd: i32, offset: i64, whence: u32) -> i64 {
    if fd < 0 || fd >= MAX_OPEN_FILES as i32 || !FD_TABLE[fd as usize].in_use {
        return -1;
    }

    let node = FD_TABLE[fd as usize].node;
    if node.is_null() {
        return -1;
    }

    let new_off: i64 = match whence {
        0 => {
            // SEEK_SET
            offset
        }
        1 => {
            // SEEK_CUR
            FD_TABLE[fd as usize].offset as i64 + offset
        }
        2 => {
            // SEEK_END
            (*node).size as i64 + offset
        }
        _ => return -1,
    };

    if new_off < 0 {
        return -1;
    }

    FD_TABLE[fd as usize].offset = new_off as u32;
    new_off
}

// ---- Allocate a directory node -----------------------------------------------

unsafe fn make_dir(name: *const u8) -> *mut VfsNode {
    let dir = heap::kmalloc(core::mem::size_of::<VfsNode>()) as *mut VfsNode;
    if dir.is_null() {
        return core::ptr::null_mut();
    }
    string::memset(dir as *mut u8, 0, core::mem::size_of::<VfsNode>());

    let len = string::strlen(name);
    let copy_len = if len > 63 { 63 } else { len };
    string::memcpy((*dir).name.as_mut_ptr(), name, copy_len);
    (*dir).name[copy_len] = 0;

    (*dir).flags = VFS_DIRECTORY;
    (*dir).readdir_fn = Some(default_readdir);
    (*dir).finddir_fn = Some(default_finddir);
    (*dir).num_children = 0;
    (*dir).parent = core::ptr::null_mut();
    dir
}

// ---- finddir_root: convenience for finding a child in root -------------------

/// Look up a file by name (null-terminated) directly in the VFS root.
/// Returns Some(pointer) or None if not found.
pub unsafe fn finddir_root(name: *const u8) -> Option<*mut VfsNode> {
    let node = finddir(VFS_ROOT, name);
    if node.is_null() {
        None
    } else {
        Some(node)
    }
}

// ---- create_file: delegates to ramfs -----------------------------------------

pub unsafe fn create_file(name: *const u8) -> *mut VfsNode {
    crate::ramfs::create_file(name)
}

// ---- Initialization ----------------------------------------------------------

pub unsafe fn init() {
    // Clear fd table
    for i in 0..MAX_OPEN_FILES {
        FD_TABLE[i].node = core::ptr::null_mut();
        FD_TABLE[i].offset = 0;
        FD_TABLE[i].in_use = false;
    }

    // Create root directory "/"
    VFS_ROOT = make_dir(b"/\0".as_ptr());
    if VFS_ROOT.is_null() {
        vga::puts(b"VFS: fatal: failed to allocate root directory\n");
        return;
    }

    // Populate from initrd
    crate::initrd::init_vfs(VFS_ROOT);

    // Create /dev
    crate::devfs::init(VFS_ROOT);

    // Initialize RAM filesystem
    crate::ramfs::init();

    vga::puts(b"VFS: initialized\n");
}
