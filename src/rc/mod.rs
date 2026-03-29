// rc/mod.rs -- Rust subset compiler driver.
//
// Reads a .rs source file from the VFS, initializes all compiler
// subsystems (emitter, symbol table, lexer), runs the parser/codegen,
// and writes the resulting flat binary to a new VFS file.

pub mod emit;
pub mod lex;
pub mod sym;
pub mod parse;

use crate::heap;
use crate::string;
use crate::vfs;
use crate::vga;

// Print an unsigned integer to the VGA console
unsafe fn cc_print_uint(val: u32) {
    let mut buf = [0u8; 12];
    let mut i = 0usize;
    if val == 0 {
        vga::putchar(b'0');
        return;
    }
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

// Derive output filename from source filename (replace .rs with .bin)
unsafe fn derive_output_name(src_name: *const u8, out: *mut u8, out_size: i32) {
    let mut len = string::strlen(src_name) as i32;
    if len >= out_size - 4 {
        len = out_size - 5;
    }

    string::memcpy(out, src_name, len as usize);

    // Find last '.'
    let mut idx = len;
    while idx > 0 && *out.offset((idx - 1) as isize) != b'.' {
        idx -= 1;
    }

    if idx > 0 {
        // Replace extension
        *out.offset((idx - 1) as isize) = b'.';
        *out.offset(idx as isize) = b'b';
        *out.offset((idx + 1) as isize) = b'i';
        *out.offset((idx + 2) as isize) = b'n';
        *out.offset((idx + 3) as isize) = 0;
    } else {
        // No extension -- append .bin
        *out.offset(len as isize) = b'.';
        *out.offset((len + 1) as isize) = b'b';
        *out.offset((len + 2) as isize) = b'i';
        *out.offset((len + 3) as isize) = b'n';
        *out.offset((len + 4) as isize) = 0;
    }
}

// Compile a source file to a flat binary.
// source_file: name of the .rs file in the VFS root directory
// output_file: name for the output binary (null = auto-derive from source)
// Returns 0 on success, -1 on error.
pub unsafe fn rc_compile(source_file: *const u8, output_file: *const u8) -> i32 {
    // Locate and read the source file
    let src_node = vfs::finddir(vfs::VFS_ROOT, source_file);
    if src_node.is_null() {
        vga::puts(b"rc:file not found: ");
        vga::puts(core::slice::from_raw_parts(
            source_file,
            string::strlen(source_file),
        ));
        vga::putchar(b'\n');
        return -1;
    }

    let source = heap::kmalloc((*src_node).size as usize + 1);
    if source.is_null() {
        vga::puts(b"rc:out of memory\n");
        return -1;
    }

    let n = vfs::read(src_node, 0, (*src_node).size, source);
    if n < 0 {
        vga::puts(b"rc:read error\n");
        heap::kfree(source);
        return -1;
    }
    *source.offset(n as isize) = 0;

    // Initialize compiler subsystems
    emit::init();
    sym::init();
    lex::init(source, n);
    lex::next(); // prime the first token

    // Parse and generate code
    if parse::parse_program() < 0 {
        vga::puts(b"rc:compilation failed\n");
        heap::kfree(source);
        return -1;
    }

    // Report code buffer overflow
    if emit::had_overflow() {
        vga::puts(b"rc:error: code buffer overflow (output too large)\n");
        heap::kfree(source);
        return -1;
    }

    // Determine output filename
    let mut out_name = [0u8; 64];
    let actual_output: *const u8;
    if output_file.is_null() {
        derive_output_name(source_file, out_name.as_mut_ptr(), 64);
        actual_output = out_name.as_ptr();
    } else {
        actual_output = output_file;
    }

    // Write the generated binary to the VFS
    let out_node = vfs::create_file(actual_output);
    if out_node.is_null() {
        vga::puts(b"rc:cannot create output file: ");
        vga::puts(core::slice::from_raw_parts(
            actual_output,
            string::strlen(actual_output),
        ));
        vga::putchar(b'\n');
        heap::kfree(source);
        return -1;
    }

    vfs::write(out_node, 0, emit::size(), emit::code());

    heap::kfree(source);

    // Report success
    vga::puts(b"Compiled: ");
    vga::puts(core::slice::from_raw_parts(
        actual_output,
        string::strlen(actual_output),
    ));
    vga::puts(b" (");
    cc_print_uint(emit::size());
    vga::puts(b" bytes)\n");

    0
}

// Compile a source file with auto-derived output name.
// Called from compiled programs that need to compile sub-programs.
// The caller is running at CC_LOAD_BASE. Sub-programs are compiled for
// CC_LOAD_BASE2 so run_program() can load them without overwriting the caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn compile(source_file: *const u8) -> i32 {
    emit::set_base(emit::CC_LOAD_BASE2);
    let result = rc_compile(source_file, core::ptr::null());
    emit::set_base(emit::CC_LOAD_BASE);
    result
}

// Load and run a compiled binary at CC_LOAD_BASE2 (0x00B00000).
// Called from compiled programs (e.g., test.rs at CC_LOAD_BASE).
// Sub-programs were compiled by compile() targeting CC_LOAD_BASE2,
// so all absolute addresses are correct at this load address.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn run_program(filename: *const u8) -> i32 {
    let node = vfs::finddir(vfs::VFS_ROOT, filename);
    if node.is_null() || (*node).size == 0 {
        return -1;
    }

    let load_addr = emit::CC_LOAD_BASE2 as *mut u8;
    let n = vfs::read(node, 0, (*node).size, load_addr);
    if n <= 0 {
        return -1;
    }

    let entry: extern "C" fn() = core::mem::transmute(emit::CC_LOAD_BASE2 as usize);
    crate::recovery::run_protected(entry)
}
