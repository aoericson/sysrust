// editor.rs -- Minimal line-oriented text editor (ed-style).
//
// Commands:
//   p           - print all lines
//   p N         - print line N (1-indexed)
//   d N         - delete line N (1-indexed)
//   i N         - insert before line N (end with "." on its own)
//   e N         - replace line N with new text
//   w           - save file
//   wq          - save and quit
//   q           - quit
//   <text>      - append text as a new line

use crate::vga::{self, Color};
use crate::keyboard;
use crate::string;
use crate::heap;
use crate::vfs::{self, VFS_ROOT, VfsNode};

const EDITOR_MAX_LINES: usize = 256;
const EDITOR_LINE_LEN: usize = 80;
const EDITOR_CMD_LEN: usize = 80;

/// Static buffer lives in BSS.
static mut LINES: [[u8; EDITOR_LINE_LEN]; EDITOR_MAX_LINES] =
    [[0u8; EDITOR_LINE_LEN]; EDITOR_MAX_LINES];
static mut LINE_COUNT: i32 = 0;
static mut EDIT_FILENAME: [u8; 64] = [0u8; 64];

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Print an unsigned integer without using printf.
unsafe fn ed_print_uint(val: u32) {
    if val == 0 {
        vga::putchar(b'0');
        return;
    }
    let mut buf = [0u8; 12];
    let mut i = 0usize;
    let mut v = val;
    while v > 0 {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i += 1;
    }
    while i > 0 {
        i -= 1;
        vga::putchar(buf[i]);
    }
}

/// Print a decimal integer with a fixed-width field of 3 characters,
/// right-aligned (space-padded on the left).
unsafe fn ed_print_uint3(val: i32) {
    let mut buf = [0u8; 4];
    let mut i = 0usize;
    let mut v = val;

    if v <= 0 {
        vga::puts(b"  0");
        return;
    }
    while v > 0 {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i += 1;
    }
    // pad to width 3
    while i < 3 {
        buf[i] = b' ';
        i += 1;
    }
    // buf is reversed; print it right-to-left
    while i > 0 {
        i -= 1;
        vga::putchar(buf[i]);
    }
}

/// Read one line of input into buf (max EDITOR_CMD_LEN - 1 chars).
/// Echoes characters, handles backspace, terminates on Enter.
unsafe fn editor_readline(buf: *mut u8) {
    let mut len: usize = 0;

    loop {
        let c = keyboard::getchar();

        if c == b'\n' {
            vga::putchar(b'\n');
            *buf.add(len) = 0;
            return;
        }

        if c == b'\x08' {
            if len > 0 {
                len -= 1;
                vga::putchar(b'\x08');
            }
            continue;
        }

        if len < EDITOR_CMD_LEN - 1 {
            *buf.add(len) = c;
            len += 1;
            vga::putchar(c);
        }
    }
}

// ---------------------------------------------------------------------------
// Editor operations
// ---------------------------------------------------------------------------

unsafe fn editor_print_all() {
    if LINE_COUNT == 0 {
        vga::puts(b"(empty buffer)\n");
        return;
    }
    for i in 0..LINE_COUNT as usize {
        ed_print_uint3(i as i32 + 1);
        vga::puts(b": ");
        vga::puts(&LINES[i]);
        vga::putchar(b'\n');
    }
}

unsafe fn editor_print_line(n: i32) {
    if n < 1 || n > LINE_COUNT {
        vga::puts(b"Error: line number out of range (1-");
        ed_print_uint(LINE_COUNT as u32);
        vga::puts(b")\n");
        return;
    }
    ed_print_uint3(n);
    vga::puts(b": ");
    vga::puts(&LINES[(n - 1) as usize]);
    vga::putchar(b'\n');
}

unsafe fn editor_delete_line(n: i32) {
    if n < 1 || n > LINE_COUNT {
        vga::puts(b"Error: line number out of range (1-");
        ed_print_uint(LINE_COUNT as u32);
        vga::puts(b")\n");
        return;
    }
    // Shift lines up
    let mut i = (n - 1) as usize;
    while i < (LINE_COUNT - 1) as usize {
        string::memcpy(
            LINES[i].as_mut_ptr(),
            LINES[i + 1].as_ptr(),
            EDITOR_LINE_LEN,
        );
        i += 1;
    }
    LINE_COUNT -= 1;
    vga::puts(b"Deleted line ");
    ed_print_uint(n as u32);
    vga::putchar(b'\n');
}

/// Insert mode: read lines from the user and insert them before line n.
/// Appends to end if n > line_count. Stops when user types ".".
unsafe fn editor_insert_mode(mut n: i32) {
    let mut buf = [0u8; EDITOR_CMD_LEN];

    if n < 1 {
        n = 1;
    }
    if n > LINE_COUNT + 1 {
        n = LINE_COUNT + 1;
    }
    let mut insert_at = (n - 1) as usize; // 0-indexed insertion point

    vga::puts(b"Insert mode (end with '.' on its own line):\n");

    loop {
        editor_readline(buf.as_mut_ptr());

        if string::strcmp(buf.as_ptr(), b".\0".as_ptr()) == 0 {
            break;
        }

        if LINE_COUNT >= EDITOR_MAX_LINES as i32 {
            vga::puts(b"Buffer full (max 256 lines)\n");
            break;
        }

        // Make room: shift lines from insert_at downward by one
        let mut i = LINE_COUNT as usize;
        while i > insert_at {
            string::memcpy(
                LINES[i].as_mut_ptr(),
                LINES[i - 1].as_ptr(),
                EDITOR_LINE_LEN,
            );
            i -= 1;
        }

        string::strncpy(
            LINES[insert_at].as_mut_ptr(),
            buf.as_ptr(),
            EDITOR_LINE_LEN - 1,
        );
        LINES[insert_at][EDITOR_LINE_LEN - 1] = 0;
        LINE_COUNT += 1;
        insert_at += 1;
    }
}

unsafe fn editor_save() {
    let mut node = vfs::finddir(VFS_ROOT, EDIT_FILENAME.as_ptr());
    if node.is_null() {
        node = vfs::create_file(EDIT_FILENAME.as_ptr());
    }
    if node.is_null() {
        vga::puts(b"Error: could not create file\n");
        return;
    }

    // Allocate enough for all lines + newlines + NUL
    let alloc_size = EDITOR_MAX_LINES * EDITOR_LINE_LEN;
    let buf = heap::kmalloc(alloc_size);
    if buf.is_null() {
        vga::puts(b"Error: out of memory\n");
        return;
    }

    let mut p = buf;
    let mut total_len: u32 = 0;
    for i in 0..LINE_COUNT as usize {
        let slen = string::strlen(LINES[i].as_ptr());
        string::memcpy(p, LINES[i].as_ptr(), slen);
        p = p.add(slen);
        *p = b'\n';
        p = p.add(1);
        total_len += slen as u32 + 1;
    }

    vfs::write(node, 0, total_len, buf);
    (*node).size = total_len;

    heap::kfree(buf);

    vga::puts(b"Saved: ");
    vga::puts(&EDIT_FILENAME);
    vga::puts(b" (");
    ed_print_uint(LINE_COUNT as u32);
    vga::puts(b" lines, ");
    ed_print_uint(total_len);
    vga::puts(b" bytes)\n");
}

/// Load an existing file's content into the LINES buffer.
/// Reads via vfs_read, then splits on '\n'.
unsafe fn editor_load(node: *mut VfsNode) {
    if node.is_null() || (*node).size == 0 {
        return;
    }

    let file_size = (*node).size;
    let buf = heap::kmalloc(file_size as usize + 1);
    if buf.is_null() {
        return;
    }

    let bytes_read = vfs::read(node, 0, file_size, buf);
    if bytes_read <= 0 {
        heap::kfree(buf);
        return;
    }
    *buf.add(bytes_read as usize) = 0;

    // Split on '\n'
    let mut start: usize = 0;
    let total = bytes_read as usize;
    let mut i: usize = 0;
    while i <= total && (LINE_COUNT as usize) < EDITOR_MAX_LINES {
        if *buf.add(i) == b'\n' || *buf.add(i) == 0 {
            let mut seg_len = i - start;
            if seg_len >= EDITOR_LINE_LEN {
                seg_len = EDITOR_LINE_LEN - 1;
            }
            string::memcpy(
                LINES[LINE_COUNT as usize].as_mut_ptr(),
                buf.add(start),
                seg_len,
            );
            LINES[LINE_COUNT as usize][seg_len] = 0;
            // Skip trailing empty line at EOF
            if *buf.add(i) == 0 && seg_len == 0 {
                break;
            }
            LINE_COUNT += 1;
            start = i + 1;
        }
        i += 1;
    }

    heap::kfree(buf);
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Open the editor for a given filename (null-terminated byte string).
pub unsafe fn edit(filename: *const u8) {
    // Initialise state
    string::strncpy(EDIT_FILENAME.as_mut_ptr(), filename, 63);
    EDIT_FILENAME[63] = 0;
    LINE_COUNT = 0;

    // Try to load existing file
    let existing = vfs::finddir(VFS_ROOT, filename);
    if !existing.is_null() {
        editor_load(existing);
    }

    // Banner
    vga::set_color(Color::LightCyan, Color::Black);
    vga::puts(b"Editing: ");
    vga::puts(&EDIT_FILENAME);
    vga::puts(b" (");
    ed_print_uint(LINE_COUNT as u32);
    vga::puts(b" lines)\n");
    vga::set_color(Color::LightGrey, Color::Black);
    vga::puts(b"Commands: p (print), e N (edit line), d N (delete),");
    vga::puts(b" i N (insert before), w (save), q (quit)\n");

    let mut cmd_buf = [0u8; EDITOR_CMD_LEN];

    // Editor loop
    loop {
        vga::set_color(Color::Cyan, Color::Black);
        vga::puts(b"ed:");
        ed_print_uint(LINE_COUNT as u32);
        vga::puts(b"> ");
        vga::set_color(Color::LightGrey, Color::Black);

        editor_readline(cmd_buf.as_mut_ptr());

        if string::strcmp(cmd_buf.as_ptr(), b"q\0".as_ptr()) == 0 {
            break;
        } else if string::strcmp(cmd_buf.as_ptr(), b"w\0".as_ptr()) == 0 {
            editor_save();
        } else if string::strcmp(cmd_buf.as_ptr(), b"wq\0".as_ptr()) == 0 {
            editor_save();
            break;
        } else if string::strcmp(cmd_buf.as_ptr(), b"p\0".as_ptr()) == 0 {
            editor_print_all();
        } else if cmd_buf[0] == b'p' && cmd_buf[1] == b' ' {
            let n = string::atoi(cmd_buf[2..].as_ptr());
            editor_print_line(n);
        } else if cmd_buf[0] == b'd' && cmd_buf[1] == b' ' {
            let n = string::atoi(cmd_buf[2..].as_ptr());
            editor_delete_line(n);
        } else if cmd_buf[0] == b'i' && cmd_buf[1] == b' ' {
            let n = string::atoi(cmd_buf[2..].as_ptr());
            editor_insert_mode(n);
        } else if cmd_buf[0] == b'e' && cmd_buf[1] == b' ' {
            // Replace a specific line: "e N" prints it, then reads new text
            let n = string::atoi(cmd_buf[2..].as_ptr());
            if n < 1 || n > LINE_COUNT {
                vga::puts(b"Error: line number out of range (1-");
                ed_print_uint(LINE_COUNT as u32);
                vga::puts(b")\n");
            } else {
                let mut newline = [0u8; EDITOR_CMD_LEN];
                vga::puts(b"Old: ");
                vga::puts(&LINES[(n - 1) as usize]);
                vga::puts(b"\nNew: ");
                editor_readline(newline.as_mut_ptr());
                string::strncpy(
                    LINES[(n - 1) as usize].as_mut_ptr(),
                    newline.as_ptr(),
                    EDITOR_LINE_LEN - 1,
                );
                LINES[(n - 1) as usize][EDITOR_LINE_LEN - 1] = 0;
            }
        } else {
            // Treat as line to append
            if (LINE_COUNT as usize) < EDITOR_MAX_LINES {
                string::strncpy(
                    LINES[LINE_COUNT as usize].as_mut_ptr(),
                    cmd_buf.as_ptr(),
                    EDITOR_LINE_LEN - 1,
                );
                LINES[LINE_COUNT as usize][EDITOR_LINE_LEN - 1] = 0;
                LINE_COUNT += 1;
            } else {
                vga::puts(b"Buffer full (max 256 lines)\n");
            }
        }
    }
}
