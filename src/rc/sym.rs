// rc/sym.rs -- Symbol table for the Rust subset compiler.
//
// A flat array of symbols with scope depth tracking.  On entering
// a new scope the depth increases; on leaving, all symbols at the
// current depth are removed.  Lookup walks from newest to oldest
// so inner scopes shadow outer ones.
//
// init() pre-populates the table with kernel built-in functions
// so compiled Rust code can call puts(), malloc(), etc.

use crate::string;
use crate::vga;

// Symbol kinds
pub const SYM_LOCAL: i32 = 0;
pub const SYM_GLOBAL: i32 = 1;
pub const SYM_FUNC: i32 = 2;
pub const SYM_KERN_FUNC: i32 = 3;
pub const SYM_PARAM: i32 = 4;
pub const SYM_CONST: i32 = 5;

// Symbol types
pub const TYPE_INT: i32 = 0;
pub const TYPE_CHAR: i32 = 1;
pub const TYPE_VOID: i32 = 2;
pub const TYPE_PTR: i32 = 3;

pub const MAX_SYMBOLS: usize = 256;

#[repr(C)]
pub struct Symbol {
    pub name: [u8; 32],
    pub kind: i32,
    pub sym_type: i32,
    pub is_ptr: i32,
    pub offset: i32,
    pub addr: u32,
    pub scope_depth: i32,
    pub struct_name: [u8; 32],
    pub is_mutable: i32,
}

impl Symbol {
    const fn new() -> Symbol {
        Symbol {
            name: [0u8; 32],
            kind: 0,
            sym_type: 0,
            is_ptr: 0,
            offset: 0,
            addr: 0,
            scope_depth: 0,
            struct_name: [0u8; 32],
            is_mutable: 0,
        }
    }
}

// Struct definitions
pub const MAX_STRUCT_FIELDS: usize = 16;
pub const MAX_STRUCT_DEFS: usize = 16;

#[repr(C)]
pub struct StructField {
    pub name: [u8; 32],
    pub field_type: i32,
    pub is_ptr: i32,
    pub offset: i32,
    pub elem_size: i32,
}

impl StructField {
    const fn new() -> StructField {
        StructField {
            name: [0u8; 32],
            field_type: 0,
            is_ptr: 0,
            offset: 0,
            elem_size: 0,
        }
    }
}

#[repr(C)]
pub struct StructDef {
    pub name: [u8; 32],
    pub fields: [StructField; MAX_STRUCT_FIELDS],
    pub field_count: i32,
    pub size: i32,
}

impl StructDef {
    const fn new() -> StructDef {
        StructDef {
            name: [0u8; 32],
            fields: [const { StructField::new() }; MAX_STRUCT_FIELDS],
            field_count: 0,
            size: 0,
        }
    }
}

static mut SYMBOLS: [Symbol; MAX_SYMBOLS] = [const { Symbol::new() }; MAX_SYMBOLS];
static mut SYM_COUNT: i32 = 0;
static mut CUR_SCOPE: i32 = 0;

static mut STRUCT_DEFS: [StructDef; MAX_STRUCT_DEFS] = [const { StructDef::new() }; MAX_STRUCT_DEFS];
static mut STRUCT_DEF_COUNT: i32 = 0;

// C-ABI wrappers for Rust functions that take non-C-compatible types.
// These are called from compiled programs via raw function pointers.

// vga::puts takes &[u8] (fat pointer), but C passes const char*.
// This wrapper constructs a slice from the null-terminated pointer.
unsafe extern "C" fn builtin_puts(s: *const u8) {
    if s.is_null() { return; }
    let len = crate::string::strlen(s);
    crate::vga::puts(core::slice::from_raw_parts(s, len));
}

// serial::puts takes &[u8], but C passes const char*.
unsafe extern "C" fn builtin_serial_puts(s: *const u8) {
    if s.is_null() { return; }
    let len = crate::string::strlen(s);
    crate::serial::puts(core::slice::from_raw_parts(s, len));
}

// vga::set_color takes Color enums; C passes two u8 values.
unsafe extern "C" fn builtin_set_color(fg: u8, bg: u8) {
    crate::vga::set_color(
        core::mem::transmute(fg),
        core::mem::transmute(bg),
    );
}

// recovery::run_protected takes extern "C" fn(), C passes a raw pointer.
unsafe extern "C" fn builtin_run_protected(entry: extern "C" fn()) -> i32 {
    crate::recovery::run_protected(entry)
}

// Add a kernel built-in function to the symbol table
unsafe fn add_builtin(name: &[u8], addr: u32) {
    add(name.as_ptr(), SYM_KERN_FUNC, TYPE_INT, 0, 0, addr);
}

// Initialize the symbol table and register kernel built-ins
pub unsafe fn init() {
    SYM_COUNT = 0;
    CUR_SCOPE = 0;
    STRUCT_DEF_COUNT = 0;
    string::memset(
        SYMBOLS.as_mut_ptr() as *mut u8,
        0,
        core::mem::size_of::<[Symbol; MAX_SYMBOLS]>(),
    );
    string::memset(
        STRUCT_DEFS.as_mut_ptr() as *mut u8,
        0,
        core::mem::size_of::<[StructDef; MAX_STRUCT_DEFS]>(),
    );

    add_builtin(b"puts\0", builtin_puts as *const () as u32);
    add_builtin(b"putchar\0", crate::vga::putchar as *const () as u32);
    add_builtin(b"getchar\0", crate::keyboard::getchar as *const () as u32);
    add_builtin(b"sleep\0", crate::timer::wait as *const () as u32);
    add_builtin(b"malloc\0", crate::heap::kmalloc as *const () as u32);
    add_builtin(b"free\0", crate::heap::kfree as *const () as u32);
    add_builtin(b"strlen\0", crate::string::strlen as *const () as u32);
    add_builtin(b"strcmp\0", crate::string::strcmp as *const () as u32);
    add_builtin(b"strcpy\0", crate::string::strcpy as *const () as u32);
    add_builtin(b"memset\0", crate::string::memset as *const () as u32);
    add_builtin(b"memcpy\0", crate::string::memcpy as *const () as u32);

    // Serial output (for test automation)
    add_builtin(b"serial_puts\0", builtin_serial_puts as *const () as u32);
    add_builtin(b"serial_putchar\0", crate::serial::putchar as *const () as u32);

    // Compiler and runner
    add_builtin(b"compile\0", super::compile as *const () as u32);
    add_builtin(b"run_program\0", super::run_program as *const () as u32);
    add_builtin(b"run_protected\0", builtin_run_protected as *const () as u32);

    // Display
    add_builtin(b"clear\0", crate::vga::clear as *const () as u32);
    add_builtin(b"set_color\0", builtin_set_color as *const () as u32);

    // File I/O
    add_builtin(b"open\0", crate::vfs::vfs_open as *const () as u32);
    add_builtin(b"read\0", crate::vfs::vfs_fd_read as *const () as u32);
    add_builtin(b"fwrite\0", crate::vfs::vfs_fd_write as *const () as u32);
    add_builtin(b"close\0", crate::vfs::vfs_close as *const () as u32);
    add_builtin(b"create_file\0", crate::vfs::create_file as *const () as u32);

    // Timing
    add_builtin(b"get_ticks\0", crate::timer::get_ticks as *const () as u32);

    // System info
    add_builtin(b"heap_free\0", crate::heap::get_free as *const () as u32);
    add_builtin(b"free_pages\0", crate::pmm::get_free_pages as *const () as u32);

    // Networking
    add_builtin(b"ip_to_string\0", crate::net::ip_to_str as *const () as u32);
    add_builtin(b"tcp_connect\0", crate::tcp::tcp_connect as *const () as u32);
    add_builtin(b"tcp_send\0", crate::tcp::tcp_send as *const () as u32);
    add_builtin(b"tcp_recv\0", crate::tcp::tcp_recv as *const () as u32);
    add_builtin(b"tcp_close\0", crate::tcp::tcp_close as *const () as u32);
}

// Add a symbol; returns 0 on success, -1 if table is full
pub unsafe fn add(
    name: *const u8,
    kind: i32,
    sym_type: i32,
    is_ptr: i32,
    offset: i32,
    addr: u32,
) -> i32 {
    if SYM_COUNT >= MAX_SYMBOLS as i32 {
        vga::puts(b"rc: symbol table full\n");
        return -1;
    }

    let s = &mut SYMBOLS[SYM_COUNT as usize];
    SYM_COUNT += 1;
    string::strncpy(s.name.as_mut_ptr(), name, 31);
    s.name[31] = 0;
    s.kind = kind;
    s.sym_type = sym_type;
    s.is_ptr = is_ptr;
    s.offset = offset;
    s.addr = addr;
    s.scope_depth = CUR_SCOPE;
    s.struct_name[0] = 0;
    0
}

// Look up a symbol by name (most recent first, for scoping)
pub unsafe fn lookup(name: *const u8) -> *mut Symbol {
    let mut i = SYM_COUNT - 1;
    while i >= 0 {
        if string::strcmp(SYMBOLS[i as usize].name.as_ptr(), name) == 0 {
            return &mut SYMBOLS[i as usize] as *mut Symbol;
        }
        i -= 1;
    }
    core::ptr::null_mut()
}

// Enter a new scope level
pub unsafe fn enter_scope() {
    CUR_SCOPE += 1;
}

// Leave the current scope, removing all symbols defined in it
pub unsafe fn leave_scope() {
    while SYM_COUNT > 0
        && SYMBOLS[(SYM_COUNT - 1) as usize].scope_depth == CUR_SCOPE
    {
        SYM_COUNT -= 1;
    }
    if CUR_SCOPE > 0 {
        CUR_SCOPE -= 1;
    }
}

// ---- Struct definition management ----

// Register a new struct type; returns pointer or null if table full
pub unsafe fn struct_def_add(name: *const u8) -> *mut StructDef {
    if STRUCT_DEF_COUNT >= MAX_STRUCT_DEFS as i32 {
        vga::puts(b"rc: struct definition table full\n");
        return core::ptr::null_mut();
    }
    let d = &mut STRUCT_DEFS[STRUCT_DEF_COUNT as usize];
    STRUCT_DEF_COUNT += 1;
    string::strncpy(d.name.as_mut_ptr(), name, 31);
    d.name[31] = 0;
    d.field_count = 0;
    d.size = 0;
    d as *mut StructDef
}

// Find a struct definition by name; returns pointer or null
pub unsafe fn struct_def_lookup(name: *const u8) -> *mut StructDef {
    for i in 0..STRUCT_DEF_COUNT as usize {
        if string::strcmp(STRUCT_DEFS[i].name.as_ptr(), name) == 0 {
            return &mut STRUCT_DEFS[i] as *mut StructDef;
        }
    }
    core::ptr::null_mut()
}

// Find a field within a struct; returns pointer or null
pub unsafe fn struct_field_lookup(
    def: *mut StructDef,
    fieldname: *const u8,
) -> *mut StructField {
    if def.is_null() {
        return core::ptr::null_mut();
    }
    for i in 0..(*def).field_count as usize {
        if string::strcmp((*def).fields[i].name.as_ptr(), fieldname) == 0 {
            return &mut (*def).fields[i] as *mut StructField;
        }
    }
    core::ptr::null_mut()
}
