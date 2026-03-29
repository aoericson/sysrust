// devfs.rs -- Device filesystem (/dev).
//
// Provides virtual device nodes:
//   /dev/null    -- read returns EOF, write discards data
//   /dev/zero    -- read fills buffer with zeros, write discards data
//   /dev/console -- read calls keyboard::getchar, write calls vga::putchar

use crate::heap;
use crate::string;
use crate::vga;
use crate::keyboard;
use crate::vfs::{self, VfsNode, VFS_FILE, VFS_DIRECTORY, VFS_DEVICE};

// ---- /dev/null ---------------------------------------------------------------

fn null_read(_node: *mut VfsNode, _offset: u32, _size: u32, _buffer: *mut u8) -> i32 {
    0 // EOF
}

fn null_write(_node: *mut VfsNode, _offset: u32, size: u32, _buffer: *const u8) -> i32 {
    size as i32 // accept everything
}

// ---- /dev/zero ---------------------------------------------------------------

fn zero_read(_node: *mut VfsNode, _offset: u32, size: u32, buffer: *mut u8) -> i32 {
    unsafe {
        string::memset(buffer, 0, size as usize);
    }
    size as i32
}

fn zero_write(_node: *mut VfsNode, _offset: u32, size: u32, _buffer: *const u8) -> i32 {
    size as i32 // discard
}

// ---- /dev/console ------------------------------------------------------------

fn console_read(_node: *mut VfsNode, _offset: u32, size: u32, buffer: *mut u8) -> i32 {
    unsafe {
        let mut i: u32 = 0;
        while i < size {
            let c = keyboard::getchar() as u8;
            *buffer.add(i as usize) = c;
            // Return on newline so the caller gets one line at a time
            if c == b'\n' {
                i += 1;
                break;
            }
            i += 1;
        }
        i as i32
    }
}

fn console_write(_node: *mut VfsNode, _offset: u32, size: u32, buffer: *const u8) -> i32 {
    unsafe {
        for i in 0..size as usize {
            vga::putchar(*buffer.add(i));
        }
    }
    size as i32
}

// ---- Directory operations for /dev -------------------------------------------

fn devfs_readdir(dir: *mut VfsNode, index: u32) -> *mut VfsNode {
    unsafe {
        if index >= (*dir).num_children {
            return core::ptr::null_mut();
        }
        (*dir).children[index as usize]
    }
}

fn devfs_finddir(dir: *mut VfsNode, name: *const u8) -> *mut VfsNode {
    unsafe {
        for i in 0..(*dir).num_children as usize {
            if string::strcmp((*(*dir).children[i]).name.as_ptr(), name) == 0 {
                return (*dir).children[i];
            }
        }
        core::ptr::null_mut()
    }
}

// ---- Helper: create a device node --------------------------------------------

unsafe fn make_device(
    name: *const u8,
    rfn: Option<fn(*mut VfsNode, u32, u32, *mut u8) -> i32>,
    wfn: Option<fn(*mut VfsNode, u32, u32, *const u8) -> i32>,
) -> *mut VfsNode {
    let node = heap::kmalloc(core::mem::size_of::<VfsNode>()) as *mut VfsNode;
    if node.is_null() {
        return core::ptr::null_mut();
    }
    string::memset(node as *mut u8, 0, core::mem::size_of::<VfsNode>());

    let len = string::strlen(name);
    let copy_len = if len > 63 { 63 } else { len };
    string::memcpy((*node).name.as_mut_ptr(), name, copy_len);
    (*node).name[copy_len] = 0;

    (*node).flags = VFS_FILE | VFS_DEVICE;
    (*node).read_fn = rfn;
    (*node).write_fn = wfn;
    node
}

// ---- Initialization ----------------------------------------------------------

/// Create /dev directory with null, zero, and console device nodes.
pub unsafe fn init(root: *mut VfsNode) {
    // Create /dev directory node
    let dev_dir = heap::kmalloc(core::mem::size_of::<VfsNode>()) as *mut VfsNode;
    if dev_dir.is_null() {
        return;
    }
    string::memset(dev_dir as *mut u8, 0, core::mem::size_of::<VfsNode>());
    string::memcpy((*dev_dir).name.as_mut_ptr(), b"dev\0".as_ptr(), 4);

    (*dev_dir).flags = VFS_DIRECTORY;
    (*dev_dir).readdir_fn = Some(devfs_readdir);
    (*dev_dir).finddir_fn = Some(devfs_finddir);
    (*dev_dir).num_children = 0;

    // Add device nodes to /dev
    let null_node = make_device(b"null\0".as_ptr(), Some(null_read), Some(null_write));
    let zero_node = make_device(b"zero\0".as_ptr(), Some(zero_read), Some(zero_write));
    let console_node = make_device(
        b"console\0".as_ptr(),
        Some(console_read),
        Some(console_write),
    );

    vfs::add_child(dev_dir, null_node);
    vfs::add_child(dev_dir, zero_node);
    vfs::add_child(dev_dir, console_node);

    // Attach /dev to root
    vfs::add_child(root, dev_dir);
}
