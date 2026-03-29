// vfs.rs -- Virtual File System core.
//
// Manages the root directory, dispatches operations through node function
// pointers, and maintains a global file descriptor table.

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
}

// Global root directory
pub static mut VFS_ROOT: *mut VfsNode = core::ptr::null_mut();

// File descriptor table
struct FdEntry {
    node: *mut VfsNode,
    offset: u32,
    in_use: bool,
}

static mut FD_TABLE: [FdEntry; MAX_OPEN_FILES] = {
    const EMPTY: FdEntry = FdEntry {
        node: core::ptr::null_mut(),
        offset: 0,
        in_use: false,
    };
    [EMPTY; MAX_OPEN_FILES]
};

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
    0
}

// ---- Path resolution ---------------------------------------------------------

unsafe fn resolve_path(path: *const u8) -> *mut VfsNode {
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
