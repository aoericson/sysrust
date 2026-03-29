// cc/mod.rs -- C subset compiler driver.
//
// Reads a .c source file from the VFS, initializes all compiler
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

// ---- #include preprocessor ----

const MAX_INCLUDE_DEPTH: i32 = 8;
const MAX_INCLUDES: usize = 32;
const MAX_INCLUDE_NAME: usize = 64;

// Track already-included files (pragma-once style dedup)
static mut INCLUDED_FILES: [[u8; MAX_INCLUDE_NAME]; MAX_INCLUDES] =
    [[0u8; MAX_INCLUDE_NAME]; MAX_INCLUDES];
static mut INCLUDED_COUNT: i32 = 0;

// Check if a file has already been included; return true if so
unsafe fn already_included(name: *const u8) -> bool {
    for i in 0..INCLUDED_COUNT as usize {
        if string::strcmp(INCLUDED_FILES[i].as_ptr(), name) == 0 {
            return true;
        }
    }
    false
}

// Mark a file as included
unsafe fn mark_included(name: *const u8) {
    if (INCLUDED_COUNT as usize) < MAX_INCLUDES {
        string::strncpy(
            INCLUDED_FILES[INCLUDED_COUNT as usize].as_mut_ptr(),
            name,
            MAX_INCLUDE_NAME - 1,
        );
        INCLUDED_FILES[INCLUDED_COUNT as usize][MAX_INCLUDE_NAME - 1] = 0;
        INCLUDED_COUNT += 1;
    }
}

// Expand all #include "file.h" directives in source text.
// Returns a new kmalloc'd buffer with includes replaced by file contents.
// Caller must kfree the result.  Returns null on error.
unsafe fn expand_includes(
    src: *const u8,
    src_len: i32,
    out_len: *mut i32,
    depth: i32,
) -> *mut u8 {
    if depth > MAX_INCLUDE_DEPTH {
        vga::puts(b"cc: #include nested too deeply\n");
        return core::ptr::null_mut();
    }

    // Scan for #include lines
    let mut i: i32 = 0;
    while i < src_len {
        // Find start of this line
        let line_start = i;

        // Skip optional leading whitespace
        let mut j = i;
        while j < src_len && (*src.offset(j as isize) == b' ' || *src.offset(j as isize) == b'\t')
        {
            j += 1;
        }

        // Check for #include "
        if j < src_len && *src.offset(j as isize) == b'#' {
            let mut k = j + 1;
            // skip whitespace after #
            while k < src_len
                && (*src.offset(k as isize) == b' ' || *src.offset(k as isize) == b'\t')
            {
                k += 1;
            }
            // Check for "include"
            if k + 7 <= src_len
                && *src.offset(k as isize) == b'i'
                && *src.offset((k + 1) as isize) == b'n'
                && *src.offset((k + 2) as isize) == b'c'
                && *src.offset((k + 3) as isize) == b'l'
                && *src.offset((k + 4) as isize) == b'u'
                && *src.offset((k + 5) as isize) == b'd'
                && *src.offset((k + 6) as isize) == b'e'
            {
                k += 7;
                // skip whitespace
                while k < src_len
                    && (*src.offset(k as isize) == b' ' || *src.offset(k as isize) == b'\t')
                {
                    k += 1;
                }
                // Expect opening quote
                if k < src_len && *src.offset(k as isize) == b'"' {
                    k += 1;
                    let name_start = k;
                    while k < src_len
                        && *src.offset(k as isize) != b'"'
                        && *src.offset(k as isize) != b'\n'
                    {
                        k += 1;
                    }
                    let name_end = k;
                    let fname_len = name_end - name_start;
                    if fname_len <= 0 || fname_len >= MAX_INCLUDE_NAME as i32 {
                        // skip to end of line
                        while i < src_len && *src.offset(i as isize) != b'\n' {
                            i += 1;
                        }
                        if i < src_len {
                            i += 1;
                        }
                        continue;
                    }
                    let mut filename = [0u8; MAX_INCLUDE_NAME];
                    string::memcpy(
                        filename.as_mut_ptr(),
                        src.offset(name_start as isize),
                        fname_len as usize,
                    );
                    filename[fname_len as usize] = 0;

                    // Find end of this #include line
                    while k < src_len && *src.offset(k as isize) != b'\n' {
                        k += 1;
                    }
                    if k < src_len {
                        k += 1; // skip the newline
                    }

                    // Dedup: skip if already included
                    if already_included(filename.as_ptr()) {
                        // Build result without this line
                        let result_len = src_len - (k - line_start);
                        let result = heap::kmalloc((result_len + 1) as usize);
                        if result.is_null() {
                            return core::ptr::null_mut();
                        }
                        string::memcpy(result, src, line_start as usize);
                        string::memcpy(
                            result.offset(line_start as isize),
                            src.offset(k as isize),
                            (src_len - k) as usize,
                        );
                        *result.offset(result_len as isize) = 0;
                        // Recurse to handle remaining includes
                        let expanded_inc = expand_includes(result, result_len, out_len, depth);
                        heap::kfree(result);
                        return expanded_inc;
                    }

                    // Read the included file from VFS
                    let inc_node = vfs::finddir(vfs::VFS_ROOT, filename.as_ptr());
                    if inc_node.is_null() {
                        vga::puts(b"cc: #include file not found: ");
                        vga::puts(&filename);
                        vga::putchar(b'\n');
                        return core::ptr::null_mut();
                    }

                    let inc_buf = heap::kmalloc((*inc_node).size as usize + 1);
                    if inc_buf.is_null() {
                        return core::ptr::null_mut();
                    }
                    let inc_len = vfs::read(inc_node, 0, (*inc_node).size, inc_buf);
                    if inc_len < 0 {
                        heap::kfree(inc_buf);
                        return core::ptr::null_mut();
                    }
                    *inc_buf.offset(inc_len as isize) = 0;

                    mark_included(filename.as_ptr());

                    // Recursively expand includes in the included file
                    let mut expanded_inc_len: i32 = 0;
                    let expanded_inc =
                        expand_includes(inc_buf, inc_len, &mut expanded_inc_len, depth + 1);
                    heap::kfree(inc_buf);
                    if expanded_inc.is_null() {
                        return core::ptr::null_mut();
                    }

                    // Build new buffer: text_before + expanded_inc + text_after
                    let result_len = line_start + expanded_inc_len + (src_len - k);
                    let result = heap::kmalloc((result_len + 1) as usize);
                    if result.is_null() {
                        heap::kfree(expanded_inc);
                        return core::ptr::null_mut();
                    }
                    string::memcpy(result, src, line_start as usize);
                    string::memcpy(
                        result.offset(line_start as isize),
                        expanded_inc,
                        expanded_inc_len as usize,
                    );
                    string::memcpy(
                        result.offset((line_start + expanded_inc_len) as isize),
                        src.offset(k as isize),
                        (src_len - k) as usize,
                    );
                    *result.offset(result_len as isize) = 0;
                    heap::kfree(expanded_inc);

                    // Recurse to handle remaining includes in the combined text
                    let expanded = expand_includes(result, result_len, out_len, depth);
                    heap::kfree(result);
                    return expanded;
                }
            }
        }

        // Advance to next line
        while i < src_len && *src.offset(i as isize) != b'\n' {
            i += 1;
        }
        if i < src_len {
            i += 1;
        }
    }

    // No #include found -- return a copy of the original
    let result = heap::kmalloc((src_len + 1) as usize);
    if result.is_null() {
        return core::ptr::null_mut();
    }
    string::memcpy(result, src, src_len as usize);
    *result.offset(src_len as isize) = 0;
    *out_len = src_len;
    result
}

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

// Derive output filename from source filename (replace .c with .bin)
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
// source_file: name of the .c file in the VFS root directory
// output_file: name for the output binary (null = auto-derive from source)
// Returns 0 on success, -1 on error.
pub unsafe fn cc_compile(source_file: *const u8, output_file: *const u8) -> i32 {
    // Locate and read the source file
    let src_node = vfs::finddir(vfs::VFS_ROOT, source_file);
    if src_node.is_null() {
        vga::puts(b"cc: file not found: ");
        vga::puts(core::slice::from_raw_parts(
            source_file,
            string::strlen(source_file),
        ));
        vga::putchar(b'\n');
        return -1;
    }

    let mut source = heap::kmalloc((*src_node).size as usize + 1);
    if source.is_null() {
        vga::puts(b"cc: out of memory\n");
        return -1;
    }

    let mut n = vfs::read(src_node, 0, (*src_node).size, source);
    if n < 0 {
        vga::puts(b"cc: read error\n");
        heap::kfree(source);
        return -1;
    }
    *source.offset(n as isize) = 0;

    // Expand #include directives before lexing
    {
        let mut expanded_len: i32 = 0;
        INCLUDED_COUNT = 0; // reset dedup table for each compilation
        let expanded = expand_includes(source, n, &mut expanded_len, 0);
        if expanded.is_null() {
            vga::puts(b"cc: include expansion failed\n");
            heap::kfree(source);
            return -1;
        }
        heap::kfree(source);
        source = expanded;
        n = expanded_len;
    }

    // Initialize compiler subsystems
    emit::init();
    sym::init();
    lex::init(source, n);
    lex::next(); // prime the first token

    // Parse and generate code
    if parse::parse_program() < 0 {
        vga::puts(b"cc: compilation failed\n");
        heap::kfree(source);
        return -1;
    }

    // Report code buffer overflow
    if emit::had_overflow() {
        vga::puts(b"cc: error: code buffer overflow (output too large)\n");
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
        vga::puts(b"cc: cannot create output file: ");
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
    let result = cc_compile(source_file, core::ptr::null());
    emit::set_base(emit::CC_LOAD_BASE);
    result
}

// Load and run a compiled binary at CC_LOAD_BASE2 (0x00B00000).
// Called from compiled programs (e.g., test.c at CC_LOAD_BASE).
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
