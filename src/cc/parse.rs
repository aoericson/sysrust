// cc/parse.rs -- Recursive-descent parser and code generator.
//
// Single-pass compilation: parses tokens and emits x86-32 machine code
// directly via the emit module.  Every expression leaves its result
// in EAX.  Binary operators use the pattern: push left, eval right to
// EAX, mov right to ECX, pop left to EAX, compute result in EAX.
//
// Supported subset: int/char/void types, single-level pointers,
// if/else, while, for, return, break, continue, function calls,
// array indexing, address-of, pointer dereference, string literals.

use crate::string;
use crate::vga;
use super::emit;
use super::emit::*;
use super::lex;
use super::lex::*;
use super::sym;
use super::sym::*;

// ---- Typedef table ----

const MAX_TYPEDEFS: usize = 32;

struct TypedefEntry {
    name: [u8; 32],
    td_type: i32,
    is_ptr: i32,
}

impl TypedefEntry {
    const fn new() -> TypedefEntry {
        TypedefEntry {
            name: [0u8; 32],
            td_type: 0,
            is_ptr: 0,
        }
    }
}

static mut TYPEDEFS: [TypedefEntry; MAX_TYPEDEFS] = [const { TypedefEntry::new() }; MAX_TYPEDEFS];
static mut TYPEDEF_COUNT: i32 = 0;

unsafe fn find_typedef(name: *const u8) -> *mut TypedefEntry {
    for i in 0..TYPEDEF_COUNT as usize {
        if string::strcmp(TYPEDEFS[i].name.as_ptr(), name) == 0 {
            return &mut TYPEDEFS[i] as *mut TypedefEntry;
        }
    }
    core::ptr::null_mut()
}

// ---- Error handling ----

static mut HAD_ERROR: bool = false;

// Minimal expression type tracking.
// 0 = not a pointer, 1 = char* (byte-sized elements), 4 = int* (dword elements).
static mut EXPR_PTR_SCALE: i32 = 0;

// Print decimal integer to console
unsafe fn print_line(line: i32) {
    let mut buf = [0u8; 12];
    let mut i = 0usize;
    let mut n = line;
    if n == 0 {
        vga::putchar(b'0');
        return;
    }
    while n > 0 {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    while i > 0 {
        i -= 1;
        vga::putchar(buf[i]);
    }
}

// Report a compile error with source line number
unsafe fn cc_error(msg: &[u8]) {
    vga::puts(b"cc: line ");
    print_line((*lex::peek()).line);
    vga::puts(b": ");
    vga::puts(msg);
    vga::putchar(b'\n');
    HAD_ERROR = true;
}

// ---- Forward-reference fixup list ----

const MAX_FIXUPS: usize = 256;

struct Fixup {
    name: [u8; 32],
    patch_pos: u32,
}

impl Fixup {
    const fn new() -> Fixup {
        Fixup {
            name: [0u8; 32],
            patch_pos: 0,
        }
    }
}

static mut FIXUPS: [Fixup; MAX_FIXUPS] = [const { Fixup::new() }; MAX_FIXUPS];
static mut FIXUP_COUNT: i32 = 0;

unsafe fn add_fixup(name: *const u8, patch_pos: u32) {
    if (FIXUP_COUNT as usize) < MAX_FIXUPS {
        string::strncpy(
            FIXUPS[FIXUP_COUNT as usize].name.as_mut_ptr(),
            name,
            31,
        );
        FIXUPS[FIXUP_COUNT as usize].name[31] = 0;
        FIXUPS[FIXUP_COUNT as usize].patch_pos = patch_pos;
        FIXUP_COUNT += 1;
    } else {
        cc_error(b"too many forward references\0");
    }
}

// ---- Global initializer table ----

const MAX_GLOBAL_INITS: usize = 128;

struct GlobalInit {
    addr: u32,
    value: i32,
}

impl GlobalInit {
    const fn new() -> GlobalInit {
        GlobalInit { addr: 0, value: 0 }
    }
}

static mut GLOBAL_INITS: [GlobalInit; MAX_GLOBAL_INITS] =
    [const { GlobalInit::new() }; MAX_GLOBAL_INITS];
static mut GLOBAL_INIT_COUNT: i32 = 0;

// ---- String literal table ----

const MAX_STRINGS: usize = 128;

struct StringEntry {
    text: [u8; 128],
    len: i32,
    patch_pos: u32,
}

impl StringEntry {
    const fn new() -> StringEntry {
        StringEntry {
            text: [0u8; 128],
            len: 0,
            patch_pos: 0,
        }
    }
}

static mut STRINGS: [StringEntry; MAX_STRINGS] = [const { StringEntry::new() }; MAX_STRINGS];
static mut STRING_COUNT: i32 = 0;

unsafe fn add_string(text: *const u8, len: i32, patch_pos: u32) {
    if (STRING_COUNT as usize) < MAX_STRINGS {
        let idx = STRING_COUNT as usize;
        string::memcpy(STRINGS[idx].text.as_mut_ptr(), text, len as usize);
        STRINGS[idx].text[len as usize] = 0;
        STRINGS[idx].len = len;
        STRINGS[idx].patch_pos = patch_pos;
        STRING_COUNT += 1;
    } else {
        cc_error(b"too many string literals\0");
    }
}

// ---- Loop context stack (for break/continue) ----

const MAX_LOOP_DEPTH: usize = 16;

struct LoopCtx {
    continue_target: u32,
    break_patches: [u32; 32],
    break_count: i32,
}

impl LoopCtx {
    const fn new() -> LoopCtx {
        LoopCtx {
            continue_target: 0,
            break_patches: [0u32; 32],
            break_count: 0,
        }
    }
}

static mut LOOP_STACK: [LoopCtx; MAX_LOOP_DEPTH] = [const { LoopCtx::new() }; MAX_LOOP_DEPTH];
static mut LOOP_DEPTH: i32 = 0;

unsafe fn loop_push(cont_target: u32) {
    if (LOOP_DEPTH as usize) < MAX_LOOP_DEPTH {
        LOOP_STACK[LOOP_DEPTH as usize].continue_target = cont_target;
        LOOP_STACK[LOOP_DEPTH as usize].break_count = 0;
        LOOP_DEPTH += 1;
    }
}

unsafe fn loop_pop() {
    if LOOP_DEPTH <= 0 {
        return;
    }
    LOOP_DEPTH -= 1;
    let ctx = &LOOP_STACK[LOOP_DEPTH as usize];
    for i in 0..ctx.break_count as usize {
        emit::patch_dword(
            ctx.break_patches[i],
            emit::pos() - (ctx.break_patches[i] + 4),
        );
    }
}

unsafe fn loop_add_break(patch_pos: u32) {
    if LOOP_DEPTH <= 0 {
        cc_error(b"break outside loop/switch\0");
        return;
    }
    let ctx = &mut LOOP_STACK[(LOOP_DEPTH - 1) as usize];
    if ctx.break_count < 32 {
        ctx.break_patches[ctx.break_count as usize] = patch_pos;
        ctx.break_count += 1;
    }
}

// ---- State ----

static mut LOCAL_OFFSET: i32 = 0;

// Track struct info for the current expression value
static mut EXPR_STRUCT_NAME: [u8; 32] = [0u8; 32];
static mut EXPR_STRUCT_IS_PTR: i32 = 0;
static mut EXPR_STRUCT_KIND: i32 = 0;
static mut EXPR_STRUCT_OFFSET: i32 = 0;
static mut EXPR_STRUCT_ADDR: u32 = 0;

// ---- Convenience ----

unsafe fn tok_is(t: i32) -> bool {
    (*lex::peek()).tok_type == t
}

// ---- After parse_type, if the type was 'struct Foo', this holds "Foo" ----
static mut PARSED_STRUCT_NAME: [u8; 32] = [0u8; 32];

// Parse a type specifier; returns true if a type was found
unsafe fn parse_type(type_out: &mut i32, is_ptr_out: &mut i32) -> bool {
    *is_ptr_out = 0;
    PARSED_STRUCT_NAME[0] = 0;

    // Skip storage-class and type qualifiers
    while tok_is(TOK_STATIC) || tok_is(TOK_CONST) || tok_is(TOK_EXTERN) {
        lex::next();
    }

    // Handle unsigned/signed/short/long qualifiers
    if tok_is(TOK_UNSIGNED) || tok_is(TOK_SIGNED) {
        lex::next();
        if tok_is(TOK_CHAR) {
            *type_out = TYPE_CHAR;
            lex::next();
        } else if tok_is(TOK_SHORT) {
            *type_out = TYPE_INT;
            lex::next();
        } else if tok_is(TOK_INT) {
            *type_out = TYPE_INT;
            lex::next();
        } else if tok_is(TOK_LONG) {
            *type_out = TYPE_INT;
            lex::next();
        } else {
            *type_out = TYPE_INT;
        }
    } else if tok_is(TOK_SHORT) {
        *type_out = TYPE_INT;
        lex::next();
        if tok_is(TOK_INT) {
            lex::next();
        }
    } else if tok_is(TOK_LONG) {
        *type_out = TYPE_INT;
        lex::next();
        if tok_is(TOK_INT) {
            lex::next();
        }
    } else if tok_is(TOK_INT) {
        *type_out = TYPE_INT;
        lex::next();
    } else if tok_is(TOK_CHAR) {
        *type_out = TYPE_CHAR;
        lex::next();
    } else if tok_is(TOK_VOID) {
        *type_out = TYPE_VOID;
        lex::next();
    } else if tok_is(TOK_STRUCT) {
        lex::next(); // consume 'struct'
        if tok_is(TOK_IDENT) {
            string::strncpy(PARSED_STRUCT_NAME.as_mut_ptr(), (*lex::peek()).str_val.as_ptr(), 31);
            PARSED_STRUCT_NAME[31] = 0;
            lex::next(); // consume struct name
        } else {
            cc_error(b"expected struct name\0");
            return false;
        }
        *type_out = TYPE_INT; // struct vars treated as int-type for storage
    } else if tok_is(TOK_IDENT) {
        // Check typedef table
        let td = find_typedef((*lex::peek()).str_val.as_ptr());
        if !td.is_null() {
            *type_out = (*td).td_type;
            *is_ptr_out = (*td).is_ptr;
            lex::next();
        } else {
            return false; // not a type
        }
    } else {
        return false;
    }

    while tok_is(TOK_STAR) {
        *is_ptr_out = 1;
        lex::next();
    }
    true
}

// Check whether current token begins a type specifier
unsafe fn is_type_token() -> bool {
    if tok_is(TOK_INT)
        || tok_is(TOK_CHAR)
        || tok_is(TOK_VOID)
        || tok_is(TOK_STRUCT)
        || tok_is(TOK_STATIC)
        || tok_is(TOK_CONST)
        || tok_is(TOK_EXTERN)
        || tok_is(TOK_UNSIGNED)
        || tok_is(TOK_SIGNED)
        || tok_is(TOK_SHORT)
        || tok_is(TOK_LONG)
        || tok_is(TOK_TYPEDEF)
    {
        return true;
    }
    // Check if current identifier is a known typedef
    if tok_is(TOK_IDENT) && !find_typedef((*lex::peek()).str_val.as_ptr()).is_null() {
        return true;
    }
    false
}

// After sym_add, set struct_name on a symbol looked up by name
unsafe fn set_sym_struct(varname: *const u8, sname: *const u8) {
    let s = sym::lookup(varname);
    if !s.is_null() {
        string::strncpy((*s).struct_name.as_mut_ptr(), sname, 31);
        (*s).struct_name[31] = 0;
    }
}

// ---- Emit a function call (shared by primary and assign paths) ----

// Emit code for reversing argc dwords on the stack (for cdecl)
unsafe fn emit_reverse_args(argc: i32) {
    for ai in 0..argc / 2 {
        let lo = ai * 4;
        let hi = (argc - 1 - ai) * 4;
        // mov eax, [esp+lo]
        emit::byte(0x8B);
        emit::byte(0x84);
        emit::byte(0x24);
        emit::dword(lo as u32);
        // mov ecx, [esp+hi]
        emit::byte(0x8B);
        emit::byte(0x8C);
        emit::byte(0x24);
        emit::dword(hi as u32);
        // mov [esp+lo], ecx
        emit::byte(0x89);
        emit::byte(0x8C);
        emit::byte(0x24);
        emit::dword(lo as u32);
        // mov [esp+hi], eax
        emit::byte(0x89);
        emit::byte(0x84);
        emit::byte(0x24);
        emit::dword(hi as u32);
    }
}

// Parse argument list and emit call; returns with result in EAX
unsafe fn emit_func_call(name: *const u8, s: *mut Symbol) {
    let mut argc: i32 = 0;

    // Parse arguments
    if !tok_is(TOK_RPAREN) {
        parse_expr();
        emit::push_reg(REG_EAX);
        argc = 1;
        while tok_is(TOK_COMMA) {
            lex::next();
            parse_expr();
            emit::push_reg(REG_EAX);
            argc += 1;
        }
        if argc > 1 {
            emit_reverse_args(argc);
        }
    }
    lex::expect(TOK_RPAREN);

    // Emit the call instruction
    if !s.is_null() && (*s).kind == SYM_KERN_FUNC {
        emit::call_abs((*s).addr);
    } else if !s.is_null() && (*s).kind == SYM_FUNC {
        let target = (*s).offset as u32;
        emit::byte(0xE8);
        let call_pos = emit::pos();
        emit::dword((target as i32 - (call_pos as i32 + 4)) as u32);
    } else {
        emit::byte(0xE8);
        let patch = emit::pos();
        emit::dword(0);
        add_fixup(name, patch);
    }

    if argc > 0 {
        emit::add_esp(argc * 4);
    }

    // Function call result: assume not a pointer
    EXPR_PTR_SCALE = 0;
    EXPR_STRUCT_NAME[0] = 0;
}

// ---- Load a symbol's value into EAX ----

unsafe fn emit_load_sym(s: *mut Symbol) {
    // Track pointer scale: 0=non-ptr, 1=char-ptr, 4=int-ptr/void-ptr
    if (*s).is_ptr != 0 {
        EXPR_PTR_SCALE = if (*s).sym_type == TYPE_CHAR { 1 } else { 4 };
    } else {
        EXPR_PTR_SCALE = 0;
    }

    // Track struct info for postfix . and -> operators
    if (*s).struct_name[0] != 0 {
        string::strncpy(EXPR_STRUCT_NAME.as_mut_ptr(), (*s).struct_name.as_ptr(), 31);
        EXPR_STRUCT_NAME[31] = 0;
        EXPR_STRUCT_IS_PTR = (*s).is_ptr;
        EXPR_STRUCT_KIND = (*s).kind;
        EXPR_STRUCT_OFFSET = (*s).offset;
        EXPR_STRUCT_ADDR = (*s).addr;
    } else {
        EXPR_STRUCT_NAME[0] = 0;
    }

    if (*s).kind == SYM_CONST {
        emit::mov_reg_imm(REG_EAX, (*s).addr);
        EXPR_PTR_SCALE = 0;
        return;
    }

    if (*s).kind == SYM_LOCAL || (*s).kind == SYM_PARAM {
        emit::load_local(REG_EAX, (*s).offset);
    } else if (*s).kind == SYM_GLOBAL {
        emit::load_global(REG_EAX, (*s).addr);
    } else if (*s).kind == SYM_FUNC {
        emit::mov_reg_imm(REG_EAX, emit::get_base() + (*s).offset as u32);
    } else if (*s).kind == SYM_KERN_FUNC {
        emit::mov_reg_imm(REG_EAX, (*s).addr);
    }
}

// ---- Store EAX into a symbol's location ----

unsafe fn emit_store_sym(s: *mut Symbol) {
    if (*s).kind == SYM_LOCAL || (*s).kind == SYM_PARAM {
        emit::store_local((*s).offset, REG_EAX);
    } else if (*s).kind == SYM_GLOBAL {
        emit::store_global((*s).addr, REG_EAX);
    }
}

// ---- Expression parsing ----

// Top-level expression
unsafe fn parse_expr() {
    if HAD_ERROR {
        return;
    }

    // Pattern: *ident = expr  (pointer dereference assignment)
    if tok_is(TOK_STAR) {
        lex::next(); // consume '*'

        if tok_is(TOK_IDENT) {
            let psym = sym::lookup((*lex::peek()).str_val.as_ptr());
            if !psym.is_null() {
                lex::next(); // consume ident

                if tok_is(TOK_ASSIGN) {
                    // *ident = expr
                    lex::next();
                    emit_load_sym(psym);
                    emit::push_reg(REG_EAX);
                    parse_expr();
                    emit::pop_reg(REG_ECX);
                    if (*psym).sym_type == TYPE_CHAR && (*psym).is_ptr != 0 {
                        emit::store_indirect_byte(REG_ECX, REG_EAX);
                    } else {
                        emit::store_indirect(REG_ECX, REG_EAX);
                    }
                    return;
                }

                // Not assignment: *ident used as rvalue (dereference read)
                emit_load_sym(psym);
                if (*psym).sym_type == TYPE_CHAR && (*psym).is_ptr != 0 {
                    emit::load_indirect_byte(REG_EAX, REG_EAX);
                } else {
                    emit::load_indirect(REG_EAX, REG_EAX);
                }
                EXPR_PTR_SCALE = 0;

                parse_binops();
                return;
            }
        }

        // General case: *(expr) -- could be read or write
        parse_unary();

        if tok_is(TOK_ASSIGN) {
            let save_scale = EXPR_PTR_SCALE;
            lex::next();
            emit::push_reg(REG_EAX);
            parse_expr();
            emit::pop_reg(REG_ECX);
            if save_scale == 1 {
                emit::store_indirect_byte(REG_ECX, REG_EAX);
            } else {
                emit::store_indirect(REG_ECX, REG_EAX);
            }
            return;
        }

        // Not assignment: dereference as rvalue
        if EXPR_PTR_SCALE == 1 {
            emit::load_indirect_byte(REG_EAX, REG_EAX);
        } else {
            emit::load_indirect(REG_EAX, REG_EAX);
        }
        EXPR_PTR_SCALE = 0;
        parse_binops();
        return;
    }

    // Pattern: ident ...
    if tok_is(TOK_IDENT) {
        let mut name = [0u8; 32];
        string::strncpy(name.as_mut_ptr(), (*lex::peek()).str_val.as_ptr(), 31);
        name[31] = 0;
        let s = sym::lookup(name.as_ptr());

        if !s.is_null() {
            lex::next(); // consume ident

            // ident = expr
            if tok_is(TOK_ASSIGN) {
                lex::next();
                parse_expr();
                emit_store_sym(s);
                return;
            }

            // ident.field or ident.field = expr (struct member access)
            if tok_is(TOK_DOT) && (*s).struct_name[0] != 0 {
                let sdef = sym::struct_def_lookup((*s).struct_name.as_ptr());
                let mut fname = [0u8; 32];
                lex::next(); // consume '.'
                if !tok_is(TOK_IDENT) {
                    cc_error(b"expected field name after '.'\0");
                    return;
                }
                string::strncpy(fname.as_mut_ptr(), (*lex::peek()).str_val.as_ptr(), 31);
                fname[31] = 0;
                lex::next();
                let fld = sym::struct_field_lookup(sdef, fname.as_ptr());
                if fld.is_null() {
                    cc_error(b"unknown struct field\0");
                    return;
                }
                // Get address of struct variable
                if (*s).kind == SYM_LOCAL || (*s).kind == SYM_PARAM {
                    // lea eax, [ebp+offset]
                    emit::byte(0x8D);
                    emit::byte(0x85);
                    emit::dword((*s).offset as u32);
                } else if (*s).kind == SYM_GLOBAL {
                    emit::mov_reg_imm(REG_EAX, (*s).addr);
                }
                // Add field offset
                if (*fld).offset > 0 {
                    emit::mov_reg_imm(REG_ECX, (*fld).offset as u32);
                    emit::add(REG_EAX, REG_ECX);
                }
                if tok_is(TOK_ASSIGN) {
                    // ident.field = expr
                    lex::next();
                    emit::push_reg(REG_EAX);
                    parse_expr();
                    emit::pop_reg(REG_ECX);
                    emit::store_indirect(REG_ECX, REG_EAX);
                    return;
                }
                // Read field value
                emit::load_indirect(REG_EAX, REG_EAX);
                EXPR_PTR_SCALE = if (*fld).is_ptr != 0 {
                    if (*fld).field_type == TYPE_CHAR { 1 } else { 4 }
                } else {
                    0
                };
                parse_binops();
                return;
            }

            // ptr->field or ptr->field = expr (struct pointer member)
            if tok_is(TOK_ARROW) && (*s).struct_name[0] != 0 {
                let sdef = sym::struct_def_lookup((*s).struct_name.as_ptr());
                let mut fname = [0u8; 32];
                lex::next(); // consume '->'
                if !tok_is(TOK_IDENT) {
                    cc_error(b"expected field name after '->'\0");
                    return;
                }
                string::strncpy(fname.as_mut_ptr(), (*lex::peek()).str_val.as_ptr(), 31);
                fname[31] = 0;
                lex::next();
                let fld = sym::struct_field_lookup(sdef, fname.as_ptr());
                if fld.is_null() {
                    cc_error(b"unknown struct field\0");
                    return;
                }
                // Load pointer value
                emit_load_sym(s);
                // Add field offset
                if (*fld).offset > 0 {
                    emit::mov_reg_imm(REG_ECX, (*fld).offset as u32);
                    emit::add(REG_EAX, REG_ECX);
                }
                if tok_is(TOK_ASSIGN) {
                    // ptr->field = expr
                    lex::next();
                    emit::push_reg(REG_EAX);
                    parse_expr();
                    emit::pop_reg(REG_ECX);
                    emit::store_indirect(REG_ECX, REG_EAX);
                    return;
                }
                // Read field value
                emit::load_indirect(REG_EAX, REG_EAX);
                EXPR_PTR_SCALE = if (*fld).is_ptr != 0 {
                    if (*fld).field_type == TYPE_CHAR { 1 } else { 4 }
                } else {
                    0
                };
                parse_binops();
                return;
            }

            // ident += expr, -= expr, *= expr, /= expr
            if tok_is(TOK_PLUSEQ)
                || tok_is(TOK_MINUSEQ)
                || tok_is(TOK_STAREQ)
                || tok_is(TOK_SLASHEQ)
            {
                let op = (*lex::peek()).tok_type;
                lex::next();
                emit_load_sym(s);
                emit::push_reg(REG_EAX);
                parse_expr();
                emit::mov_reg_reg(REG_ECX, REG_EAX);
                emit::pop_reg(REG_EAX);
                // Scale RHS for pointer += and -=
                if (*s).is_ptr != 0 && (op == TOK_PLUSEQ || op == TOK_MINUSEQ) {
                    let scale: u32 = if (*s).sym_type == TYPE_CHAR { 1 } else { 4 };
                    if scale > 1 {
                        emit::mov_reg_imm(REG_EDX, scale);
                        emit::imul(REG_ECX, REG_EDX);
                    }
                }
                if op == TOK_PLUSEQ {
                    emit::add(REG_EAX, REG_ECX);
                } else if op == TOK_MINUSEQ {
                    emit::sub(REG_EAX, REG_ECX);
                } else if op == TOK_STAREQ {
                    emit::imul(REG_EAX, REG_ECX);
                } else {
                    // /=: eax has left, ecx has right
                    emit::idiv_ecx();
                }
                emit_store_sym(s);
                return;
            }

            // ident[expr] = expr  (array element assignment)
            if tok_is(TOK_LBRACKET) {
                let elem_size: i32 = if (*s).sym_type == TYPE_CHAR { 1 } else { 4 };
                // Load base pointer
                if (*s).is_ptr != 0 {
                    emit_load_sym(s);
                } else {
                    // Array on stack: address-of the array base
                    // lea eax, [ebp+offset]
                    emit::byte(0x8D);
                    emit::byte(0x85);
                    emit::dword((*s).offset as u32);
                }
                emit::push_reg(REG_EAX); // save base
                lex::next(); // consume '['
                parse_expr(); // index -> EAX
                lex::expect(TOK_RBRACKET);

                // Compute address: base + index*elem_size
                if elem_size > 1 {
                    emit::mov_reg_imm(REG_ECX, elem_size as u32);
                    emit::imul(REG_EAX, REG_ECX);
                }
                emit::mov_reg_reg(REG_ECX, REG_EAX);
                emit::pop_reg(REG_EAX);
                emit::add(REG_EAX, REG_ECX); // EAX = element addr

                if tok_is(TOK_ASSIGN) {
                    // Assignment: a[i] = expr
                    lex::next();
                    emit::push_reg(REG_EAX);
                    parse_expr();
                    emit::pop_reg(REG_ECX);
                    if elem_size == 1 {
                        emit::store_indirect_byte(REG_ECX, REG_EAX);
                    } else {
                        emit::store_indirect(REG_ECX, REG_EAX);
                    }
                    return;
                }

                // Read: a[i] (used as rvalue)
                if elem_size == 1 {
                    emit::load_indirect_byte(REG_EAX, REG_EAX);
                } else {
                    emit::load_indirect(REG_EAX, REG_EAX);
                }

                parse_binops();
                return;
            }

            // ident(...) = function call
            if tok_is(TOK_LPAREN) {
                lex::next();
                emit_func_call(name.as_ptr(), s);
                parse_binops();
                return;
            }

            // ident++ / ident-- (post-increment / post-decrement)
            if tok_is(TOK_INC) {
                let step: u32 = if (*s).is_ptr != 0 {
                    if (*s).sym_type == TYPE_CHAR { 1 } else { 4 }
                } else {
                    1
                };
                lex::next();
                emit_load_sym(s);
                emit::push_reg(REG_EAX);
                emit::mov_reg_imm(REG_ECX, step);
                emit::add(REG_EAX, REG_ECX);
                emit_store_sym(s);
                emit::pop_reg(REG_EAX); // return old value
                parse_binops();
                return;
            }
            if tok_is(TOK_DEC) {
                let step: u32 = if (*s).is_ptr != 0 {
                    if (*s).sym_type == TYPE_CHAR { 1 } else { 4 }
                } else {
                    1
                };
                lex::next();
                emit_load_sym(s);
                emit::push_reg(REG_EAX);
                emit::mov_reg_imm(REG_ECX, step);
                emit::sub(REG_EAX, REG_ECX);
                emit_store_sym(s);
                emit::pop_reg(REG_EAX);
                parse_binops();
                return;
            }

            // Just a plain identifier -- load and continue to binary ops
            emit_load_sym(s);
            parse_binops();
            return;
        }

        // Symbol not found -- might be forward-referenced function.
        lex::next(); // consume ident
        if tok_is(TOK_LPAREN) {
            lex::next();
            emit_func_call(name.as_ptr(), core::ptr::null_mut());
            parse_binops();
            return;
        }

        // Truly undefined
        cc_error(b"undefined symbol\0");
        emit::mov_reg_imm(REG_EAX, 0);
        parse_binops();
        return;
    }

    // Not an ident -- use normal precedence chain
    parse_or();
    // Check for ternary after parse_or
    if tok_is(TOK_QUESTION) {
        lex::next();
        emit::cmp_eax_imm(0);
        let false_jump = emit::jcc_placeholder(CC_E);
        parse_expr();
        lex::expect(TOK_COLON);
        let end_jump = emit::jmp_placeholder();
        emit::patch_dword(false_jump, emit::pos() - (false_jump + 4));
        parse_or();
        emit::patch_dword(end_jump, emit::pos() - (end_jump + 4));
    }
}

// Binary operator continuation after primary/prefix parsing
unsafe fn parse_binops() {
    loop {
        if HAD_ERROR {
            return;
        }

        // Ternary: cond ? true_expr : false_expr
        if tok_is(TOK_QUESTION) {
            lex::next();
            emit::cmp_eax_imm(0);
            let false_jump = emit::jcc_placeholder(CC_E);
            parse_expr();
            lex::expect(TOK_COLON);
            let end_jump = emit::jmp_placeholder();
            emit::patch_dword(false_jump, emit::pos() - (false_jump + 4));
            parse_or();
            emit::patch_dword(end_jump, emit::pos() - (end_jump + 4));
            continue;
        }

        // Logical OR
        if tok_is(TOK_OR) {
            lex::next();
            emit::cmp_eax_imm(0);
            emit::byte(0x0F);
            emit::byte(0x85);
            let skip = emit::pos();
            emit::dword(0); // jne -> set 1
            parse_or();
            emit::cmp_eax_imm(0);
            emit::setcc(CC_NE, REG_EAX);
            emit::movzx_eax_al();
            let end = emit::jmp_placeholder();
            emit::patch_dword(skip, emit::pos() - (skip + 4));
            emit::mov_reg_imm(REG_EAX, 1);
            emit::patch_dword(end, emit::pos() - (end + 4));
            continue;
        }
        if tok_is(TOK_AND) {
            lex::next();
            emit::cmp_eax_imm(0);
            let skip = emit::jcc_placeholder(CC_E);
            parse_and_expr();
            emit::cmp_eax_imm(0);
            emit::setcc(CC_NE, REG_EAX);
            emit::movzx_eax_al();
            emit::patch_dword(skip, emit::pos() - (skip + 4));
            continue;
        }
        if tok_is(TOK_PIPE) {
            lex::next();
            emit::push_reg(REG_EAX);
            parse_bitor();
            emit::mov_reg_reg(REG_ECX, REG_EAX);
            emit::pop_reg(REG_EAX);
            emit::or(REG_EAX, REG_ECX);
            continue;
        }
        if tok_is(TOK_CARET) {
            lex::next();
            emit::push_reg(REG_EAX);
            parse_bitand();
            emit::mov_reg_reg(REG_ECX, REG_EAX);
            emit::pop_reg(REG_EAX);
            emit::xor(REG_EAX, REG_ECX);
            continue;
        }
        if tok_is(TOK_AMP) {
            lex::next();
            emit::push_reg(REG_EAX);
            parse_equal();
            emit::mov_reg_reg(REG_ECX, REG_EAX);
            emit::pop_reg(REG_EAX);
            emit::and(REG_EAX, REG_ECX);
            continue;
        }
        if tok_is(TOK_EQ) || tok_is(TOK_NEQ) {
            let cc = if tok_is(TOK_EQ) { CC_E } else { CC_NE };
            lex::next();
            emit::push_reg(REG_EAX);
            parse_relational();
            emit::mov_reg_reg(REG_ECX, REG_EAX);
            emit::pop_reg(REG_EAX);
            emit::cmp(REG_EAX, REG_ECX);
            emit::setcc(cc, REG_EAX);
            emit::movzx_eax_al();
            continue;
        }
        if tok_is(TOK_LT) || tok_is(TOK_GT) || tok_is(TOK_LTE) || tok_is(TOK_GTE) {
            let cc;
            if tok_is(TOK_LT) {
                cc = CC_L;
            } else if tok_is(TOK_GT) {
                cc = CC_G;
            } else if tok_is(TOK_LTE) {
                cc = CC_LE;
            } else {
                cc = CC_GE;
            }
            lex::next();
            emit::push_reg(REG_EAX);
            parse_shift();
            emit::mov_reg_reg(REG_ECX, REG_EAX);
            emit::pop_reg(REG_EAX);
            emit::cmp(REG_EAX, REG_ECX);
            emit::setcc(cc, REG_EAX);
            emit::movzx_eax_al();
            continue;
        }
        if tok_is(TOK_LSHIFT) || tok_is(TOK_RSHIFT) {
            let is_left = tok_is(TOK_LSHIFT);
            lex::next();
            emit::push_reg(REG_EAX);
            parse_add_expr();
            emit::mov_reg_reg(REG_ECX, REG_EAX);
            emit::pop_reg(REG_EAX);
            if is_left {
                emit::shl(REG_EAX);
            } else {
                emit::shr(REG_EAX);
            }
            continue;
        }
        if tok_is(TOK_PLUS) || tok_is(TOK_MINUS) {
            let is_sub = tok_is(TOK_MINUS);
            let left_scale = EXPR_PTR_SCALE;
            EXPR_PTR_SCALE = 0;
            lex::next();
            emit::push_reg(REG_EAX);
            parse_mul();
            // Scale right operand by element size if left is a pointer
            if left_scale > 1 {
                emit::mov_reg_imm(REG_EDX, left_scale as u32);
                emit::imul(REG_EAX, REG_EDX);
            }
            emit::mov_reg_reg(REG_ECX, REG_EAX);
            emit::pop_reg(REG_EAX);
            if is_sub {
                emit::sub(REG_EAX, REG_ECX);
            } else {
                emit::add(REG_EAX, REG_ECX);
            }
            if left_scale != 0 {
                EXPR_PTR_SCALE = left_scale;
            }
            continue;
        }
        if tok_is(TOK_STAR) || tok_is(TOK_SLASH) || tok_is(TOK_PERCENT) {
            let op = (*lex::peek()).tok_type;
            lex::next();
            emit::push_reg(REG_EAX);
            parse_unary();
            emit::mov_reg_reg(REG_ECX, REG_EAX);
            emit::pop_reg(REG_EAX);
            if op == TOK_STAR {
                emit::imul(REG_EAX, REG_ECX);
            } else {
                emit::idiv_ecx();
                if op == TOK_PERCENT {
                    emit::mov_reg_reg(REG_EAX, REG_EDX);
                }
            }
            continue;
        }
        break;
    }
}

// ---- Standard precedence chain (used when not starting with ident) ----

// Logical OR
unsafe fn parse_or() {
    if HAD_ERROR {
        return;
    }
    parse_and_expr();
    while tok_is(TOK_OR) {
        lex::next();
        emit::cmp_eax_imm(0);
        emit::byte(0x0F);
        emit::byte(0x85);
        let skip = emit::pos();
        emit::dword(0);
        parse_and_expr();
        emit::cmp_eax_imm(0);
        emit::setcc(CC_NE, REG_EAX);
        emit::movzx_eax_al();
        let end = emit::jmp_placeholder();
        emit::patch_dword(skip, emit::pos() - (skip + 4));
        emit::mov_reg_imm(REG_EAX, 1);
        emit::patch_dword(end, emit::pos() - (end + 4));
    }
}

// Logical AND
unsafe fn parse_and_expr() {
    if HAD_ERROR {
        return;
    }
    parse_bitor();
    while tok_is(TOK_AND) {
        lex::next();
        emit::cmp_eax_imm(0);
        let skip = emit::jcc_placeholder(CC_E);
        parse_bitor();
        emit::cmp_eax_imm(0);
        emit::setcc(CC_NE, REG_EAX);
        emit::movzx_eax_al();
        emit::patch_dword(skip, emit::pos() - (skip + 4));
    }
}

// Bitwise OR
unsafe fn parse_bitor() {
    if HAD_ERROR {
        return;
    }
    parse_bitxor();
    while tok_is(TOK_PIPE) {
        lex::next();
        emit::push_reg(REG_EAX);
        parse_bitxor();
        emit::mov_reg_reg(REG_ECX, REG_EAX);
        emit::pop_reg(REG_EAX);
        emit::or(REG_EAX, REG_ECX);
    }
}

// Bitwise XOR
unsafe fn parse_bitxor() {
    if HAD_ERROR {
        return;
    }
    parse_bitand();
    while tok_is(TOK_CARET) {
        lex::next();
        emit::push_reg(REG_EAX);
        parse_bitand();
        emit::mov_reg_reg(REG_ECX, REG_EAX);
        emit::pop_reg(REG_EAX);
        emit::xor(REG_EAX, REG_ECX);
    }
}

// Bitwise AND
unsafe fn parse_bitand() {
    if HAD_ERROR {
        return;
    }
    parse_equal();
    while tok_is(TOK_AMP) {
        lex::next();
        emit::push_reg(REG_EAX);
        parse_equal();
        emit::mov_reg_reg(REG_ECX, REG_EAX);
        emit::pop_reg(REG_EAX);
        emit::and(REG_EAX, REG_ECX);
    }
}

// Equality: == !=
unsafe fn parse_equal() {
    if HAD_ERROR {
        return;
    }
    parse_relational();
    while tok_is(TOK_EQ) || tok_is(TOK_NEQ) {
        let cc = if tok_is(TOK_EQ) { CC_E } else { CC_NE };
        lex::next();
        emit::push_reg(REG_EAX);
        parse_relational();
        emit::mov_reg_reg(REG_ECX, REG_EAX);
        emit::pop_reg(REG_EAX);
        emit::cmp(REG_EAX, REG_ECX);
        emit::setcc(cc, REG_EAX);
        emit::movzx_eax_al();
    }
}

// Relational: < > <= >=
unsafe fn parse_relational() {
    if HAD_ERROR {
        return;
    }
    parse_shift();
    while tok_is(TOK_LT) || tok_is(TOK_GT) || tok_is(TOK_LTE) || tok_is(TOK_GTE) {
        let cc;
        if tok_is(TOK_LT) {
            cc = CC_L;
        } else if tok_is(TOK_GT) {
            cc = CC_G;
        } else if tok_is(TOK_LTE) {
            cc = CC_LE;
        } else {
            cc = CC_GE;
        }
        lex::next();
        emit::push_reg(REG_EAX);
        parse_shift();
        emit::mov_reg_reg(REG_ECX, REG_EAX);
        emit::pop_reg(REG_EAX);
        emit::cmp(REG_EAX, REG_ECX);
        emit::setcc(cc, REG_EAX);
        emit::movzx_eax_al();
    }
}

// Shift: << >>
unsafe fn parse_shift() {
    if HAD_ERROR {
        return;
    }
    parse_add_expr();
    while tok_is(TOK_LSHIFT) || tok_is(TOK_RSHIFT) {
        let is_left = tok_is(TOK_LSHIFT);
        lex::next();
        emit::push_reg(REG_EAX);
        parse_add_expr();
        emit::mov_reg_reg(REG_ECX, REG_EAX);
        emit::pop_reg(REG_EAX);
        if is_left {
            emit::shl(REG_EAX);
        } else {
            emit::shr(REG_EAX);
        }
    }
}

// Addition and subtraction with pointer arithmetic scaling
unsafe fn parse_add_expr() {
    if HAD_ERROR {
        return;
    }
    parse_mul();
    while tok_is(TOK_PLUS) || tok_is(TOK_MINUS) {
        let is_sub = tok_is(TOK_MINUS);
        let left_scale = EXPR_PTR_SCALE;
        EXPR_PTR_SCALE = 0;
        lex::next();
        emit::push_reg(REG_EAX);
        parse_mul();
        // Scale right operand by element size if left is a pointer
        if left_scale > 1 {
            emit::mov_reg_imm(REG_EDX, left_scale as u32);
            emit::imul(REG_EAX, REG_EDX);
        }
        emit::mov_reg_reg(REG_ECX, REG_EAX);
        emit::pop_reg(REG_EAX);
        if is_sub {
            emit::sub(REG_EAX, REG_ECX);
        } else {
            emit::add(REG_EAX, REG_ECX);
        }
        // Result of ptr+int is still a pointer
        if left_scale != 0 {
            EXPR_PTR_SCALE = left_scale;
        }
    }
}

// Multiplication, division, modulo
unsafe fn parse_mul() {
    if HAD_ERROR {
        return;
    }
    parse_unary();
    while tok_is(TOK_STAR) || tok_is(TOK_SLASH) || tok_is(TOK_PERCENT) {
        let op = (*lex::peek()).tok_type;
        lex::next();
        emit::push_reg(REG_EAX);
        parse_unary();
        emit::mov_reg_reg(REG_ECX, REG_EAX);
        emit::pop_reg(REG_EAX);
        if op == TOK_STAR {
            emit::imul(REG_EAX, REG_ECX);
        } else {
            emit::idiv_ecx();
            if op == TOK_PERCENT {
                emit::mov_reg_reg(REG_EAX, REG_EDX);
            }
        }
    }
}

// Unary prefix: - ! ~ * & ++ -- (type)expr
unsafe fn parse_unary() {
    if HAD_ERROR {
        return;
    }

    // Negation: -expr
    if tok_is(TOK_MINUS) {
        lex::next();
        parse_unary();
        emit::neg(REG_EAX);
        return;
    }
    // Logical NOT: !expr
    if tok_is(TOK_BANG) {
        lex::next();
        parse_unary();
        emit::cmp_eax_imm(0);
        emit::setcc(CC_E, REG_EAX);
        emit::movzx_eax_al();
        return;
    }
    // Bitwise NOT: ~expr
    if tok_is(TOK_TILDE) {
        lex::next();
        parse_unary();
        emit::not(REG_EAX);
        return;
    }
    // Dereference: *expr
    if tok_is(TOK_STAR) {
        lex::next();
        parse_unary();
        if EXPR_PTR_SCALE == 1 {
            emit::load_indirect_byte(REG_EAX, REG_EAX);
        } else {
            emit::load_indirect(REG_EAX, REG_EAX);
        }
        EXPR_PTR_SCALE = 0;
        return;
    }
    // Address-of: &ident
    if tok_is(TOK_AMP) {
        lex::next();
        if !tok_is(TOK_IDENT) {
            cc_error(b"expected identifier after '&'\0");
            return;
        }
        let s = sym::lookup((*lex::peek()).str_val.as_ptr());
        if s.is_null() {
            cc_error(b"undefined variable\0");
            lex::next();
            return;
        }
        lex::next();
        if (*s).kind == SYM_LOCAL || (*s).kind == SYM_PARAM {
            emit::byte(0x8D); // lea eax, [ebp+offset]
            emit::byte(0x85);
            emit::dword((*s).offset as u32);
        } else if (*s).kind == SYM_GLOBAL {
            emit::mov_reg_imm(REG_EAX, (*s).addr);
        }
        return;
    }
    // Pre-increment: ++ident
    if tok_is(TOK_INC) {
        lex::next();
        if !tok_is(TOK_IDENT) {
            cc_error(b"expected ident after '++'\0");
            return;
        }
        let s = sym::lookup((*lex::peek()).str_val.as_ptr());
        if s.is_null() {
            cc_error(b"undefined variable\0");
            lex::next();
            return;
        }
        lex::next();
        let step: u32 = if (*s).is_ptr != 0 {
            if (*s).sym_type == TYPE_CHAR { 1 } else { 4 }
        } else {
            1
        };
        emit_load_sym(s);
        emit::mov_reg_imm(REG_ECX, step);
        emit::add(REG_EAX, REG_ECX);
        emit_store_sym(s);
        return;
    }
    // Pre-decrement: --ident
    if tok_is(TOK_DEC) {
        lex::next();
        if !tok_is(TOK_IDENT) {
            cc_error(b"expected ident after '--'\0");
            return;
        }
        let s = sym::lookup((*lex::peek()).str_val.as_ptr());
        if s.is_null() {
            cc_error(b"undefined variable\0");
            lex::next();
            return;
        }
        lex::next();
        let step: u32 = if (*s).is_ptr != 0 {
            if (*s).sym_type == TYPE_CHAR { 1 } else { 4 }
        } else {
            1
        };
        emit_load_sym(s);
        emit::mov_reg_imm(REG_ECX, step);
        emit::sub(REG_EAX, REG_ECX);
        emit_store_sym(s);
        return;
    }

    parse_postfix();
}

// Postfix: array indexing, struct member access after a primary
unsafe fn parse_postfix() {
    if HAD_ERROR {
        return;
    }
    parse_primary();

    loop {
        if HAD_ERROR {
            return;
        }

        if tok_is(TOK_LBRACKET) {
            let save_scale = EXPR_PTR_SCALE;
            lex::next();
            emit::push_reg(REG_EAX);
            parse_expr();
            lex::expect(TOK_RBRACKET);
            // EAX = index, stack top = base pointer
            if save_scale > 1 {
                emit::mov_reg_imm(REG_ECX, save_scale as u32);
                emit::imul(REG_EAX, REG_ECX);
            }
            emit::mov_reg_reg(REG_ECX, REG_EAX);
            emit::pop_reg(REG_EAX);
            emit::add(REG_EAX, REG_ECX);
            if save_scale == 1 {
                emit::load_indirect_byte(REG_EAX, REG_EAX);
            } else {
                emit::load_indirect(REG_EAX, REG_EAX);
            }
            continue;
        }

        // ptr->field: EAX has pointer, dereference + offset + load
        if tok_is(TOK_ARROW) && EXPR_STRUCT_NAME[0] != 0 && EXPR_STRUCT_IS_PTR != 0 {
            let sdef = sym::struct_def_lookup(EXPR_STRUCT_NAME.as_ptr());
            let mut fname = [0u8; 32];
            lex::next(); // consume '->'
            if !tok_is(TOK_IDENT) {
                cc_error(b"expected field name after '->'\0");
                return;
            }
            string::strncpy(fname.as_mut_ptr(), (*lex::peek()).str_val.as_ptr(), 31);
            fname[31] = 0;
            lex::next();
            let fld = sym::struct_field_lookup(sdef, fname.as_ptr());
            if fld.is_null() {
                cc_error(b"unknown struct field\0");
                return;
            }
            // EAX already has the pointer value
            if (*fld).offset > 0 {
                emit::mov_reg_imm(REG_ECX, (*fld).offset as u32);
                emit::add(REG_EAX, REG_ECX);
            }
            // Load the field value
            emit::load_indirect(REG_EAX, REG_EAX);
            EXPR_PTR_SCALE = if (*fld).is_ptr != 0 {
                if (*fld).field_type == TYPE_CHAR { 1 } else { 4 }
            } else {
                0
            };
            EXPR_STRUCT_NAME[0] = 0;
            continue;
        }

        // var.field
        if tok_is(TOK_DOT) && EXPR_STRUCT_NAME[0] != 0 && EXPR_STRUCT_IS_PTR == 0 {
            let sdef = sym::struct_def_lookup(EXPR_STRUCT_NAME.as_ptr());
            let mut fname = [0u8; 32];
            lex::next(); // consume '.'
            if !tok_is(TOK_IDENT) {
                cc_error(b"expected field name after '.'\0");
                return;
            }
            string::strncpy(fname.as_mut_ptr(), (*lex::peek()).str_val.as_ptr(), 31);
            fname[31] = 0;
            lex::next();
            let fld = sym::struct_field_lookup(sdef, fname.as_ptr());
            if fld.is_null() {
                cc_error(b"unknown struct field\0");
                return;
            }
            // Re-compute struct address
            if EXPR_STRUCT_KIND == SYM_LOCAL || EXPR_STRUCT_KIND == SYM_PARAM {
                emit::byte(0x8D); // lea eax, [ebp+offset]
                emit::byte(0x85);
                emit::dword(EXPR_STRUCT_OFFSET as u32);
            } else if EXPR_STRUCT_KIND == SYM_GLOBAL {
                emit::mov_reg_imm(REG_EAX, EXPR_STRUCT_ADDR);
            }
            if (*fld).offset > 0 {
                emit::mov_reg_imm(REG_ECX, (*fld).offset as u32);
                emit::add(REG_EAX, REG_ECX);
            }
            emit::load_indirect(REG_EAX, REG_EAX);
            EXPR_PTR_SCALE = if (*fld).is_ptr != 0 {
                if (*fld).field_type == TYPE_CHAR { 1 } else { 4 }
            } else {
                0
            };
            EXPR_STRUCT_NAME[0] = 0;
            continue;
        }

        break;
    }
}

// Primary: literals, identifiers, function calls, grouped expressions
unsafe fn parse_primary() {
    if HAD_ERROR {
        return;
    }

    // Number literal
    if tok_is(TOK_NUM) {
        emit::mov_reg_imm(REG_EAX, (*lex::peek()).num_val as u32);
        EXPR_PTR_SCALE = 0;
        EXPR_STRUCT_NAME[0] = 0;
        lex::next();
        return;
    }

    // String literal -- result is a char pointer
    if tok_is(TOK_STR) {
        emit::byte(0xB8); // mov eax, imm32
        let patch = emit::pos();
        emit::dword(0);
        let len = string::strlen((*lex::peek()).str_val.as_ptr()) as i32;
        add_string((*lex::peek()).str_val.as_ptr(), len, patch);
        EXPR_PTR_SCALE = 1; // string literal is char*
        EXPR_STRUCT_NAME[0] = 0;
        lex::next();
        return;
    }

    // Identifier: variable load or function call
    if tok_is(TOK_IDENT) {
        let mut name = [0u8; 32];
        string::strncpy(name.as_mut_ptr(), (*lex::peek()).str_val.as_ptr(), 31);
        name[31] = 0;
        let s = sym::lookup(name.as_ptr());
        lex::next();

        // Function call?
        if tok_is(TOK_LPAREN) {
            lex::next();
            emit_func_call(name.as_ptr(), s);
            return;
        }

        // Post-increment / post-decrement
        if tok_is(TOK_INC) && !s.is_null() {
            lex::next();
            emit_load_sym(s);
            emit::push_reg(REG_EAX);
            emit::mov_reg_imm(REG_ECX, 1);
            emit::add(REG_EAX, REG_ECX);
            emit_store_sym(s);
            emit::pop_reg(REG_EAX);
            return;
        }
        if tok_is(TOK_DEC) && !s.is_null() {
            lex::next();
            emit_load_sym(s);
            emit::push_reg(REG_EAX);
            emit::mov_reg_imm(REG_ECX, 1);
            emit::sub(REG_EAX, REG_ECX);
            emit_store_sym(s);
            emit::pop_reg(REG_EAX);
            return;
        }

        // Simple variable load
        if !s.is_null() {
            emit_load_sym(s);
        } else {
            cc_error(b"undefined symbol\0");
            emit::mov_reg_imm(REG_EAX, 0);
        }
        return;
    }

    // Grouped expression or cast: ( expr ) or ( type ) expr
    if tok_is(TOK_LPAREN) {
        lex::next();
        if is_type_token() {
            let mut ct = 0i32;
            let mut cp = 0i32;
            parse_type(&mut ct, &mut cp);
            lex::expect(TOK_RPAREN);
            parse_unary();
            return;
        }
        parse_expr();
        lex::expect(TOK_RPAREN);
        return;
    }

    cc_error(b"expected expression\0");
}

// ---- Enum parsing ----

// Parse an enum declaration: enum [name] { IDENT [= val], ... };
unsafe fn parse_enum() {
    let mut value: i32 = 0;

    lex::next(); // consume 'enum'

    // Optional enum name -- skip it
    if tok_is(TOK_IDENT) {
        lex::next();
    }

    lex::expect(TOK_LBRACE);

    while !tok_is(TOK_RBRACE) && !tok_is(TOK_EOF) && !HAD_ERROR {
        let mut name = [0u8; 32];

        if !tok_is(TOK_IDENT) {
            cc_error(b"expected enum constant name\0");
            break;
        }
        string::strncpy(name.as_mut_ptr(), (*lex::peek()).str_val.as_ptr(), 31);
        name[31] = 0;
        lex::next();

        // Optional explicit value: = N
        if tok_is(TOK_ASSIGN) {
            lex::next();
            if !tok_is(TOK_NUM) {
                cc_error(b"expected integer value\0");
                break;
            }
            value = (*lex::peek()).num_val;
            lex::next();
        }

        // Add as compile-time constant
        sym::add(name.as_ptr(), SYM_CONST, TYPE_INT, 0, 0, value as u32);
        value += 1;

        // Comma between constants (optional before closing brace)
        if tok_is(TOK_COMMA) {
            lex::next();
        }
    }

    lex::expect(TOK_RBRACE);
    lex::expect(TOK_SEMI);
}

// ---- Switch/case ----

unsafe fn parse_switch() {
    let old_local_offset = LOCAL_OFFSET;

    lex::next(); // consume 'switch'
    lex::expect(TOK_LPAREN);
    parse_expr(); // result in EAX
    lex::expect(TOK_RPAREN);

    // Allocate a temporary local for the switch value
    LOCAL_OFFSET -= 4;
    let switch_local = LOCAL_OFFSET;
    emit::store_local(switch_local, REG_EAX);

    lex::expect(TOK_LBRACE);

    // Push a context for break (reuse loop_stack; continue_target=0)
    loop_push(0);

    while !tok_is(TOK_RBRACE) && !tok_is(TOK_EOF) && !HAD_ERROR {
        if tok_is(TOK_CASE) {
            let mut is_neg = false;
            lex::next(); // consume 'case'

            // Parse case value: optional minus, then integer or enum constant
            if tok_is(TOK_MINUS) {
                is_neg = true;
                lex::next();
            }
            let mut case_val: i32;
            if tok_is(TOK_NUM) {
                case_val = (*lex::peek()).num_val;
                lex::next();
            } else if tok_is(TOK_IDENT) {
                let cs = sym::lookup((*lex::peek()).str_val.as_ptr());
                if !cs.is_null() && (*cs).kind == SYM_CONST {
                    case_val = (*cs).addr as i32;
                } else {
                    cc_error(b"expected constant in case\0");
                    break;
                }
                lex::next();
            } else {
                cc_error(b"expected constant in case\0");
                break;
            }
            if is_neg {
                case_val = -case_val;
            }

            lex::expect(TOK_COLON);

            // cmp [switch_local], case_val; jne skip
            emit::load_local(REG_EAX, switch_local);
            emit::cmp_eax_imm(case_val);
            let skip = emit::jcc_placeholder(CC_NE);

            // Parse the case body until next case/default/rbrace
            while !tok_is(TOK_CASE)
                && !tok_is(TOK_DEFAULT)
                && !tok_is(TOK_RBRACE)
                && !tok_is(TOK_EOF)
                && !HAD_ERROR
            {
                parse_stmt();
            }

            // Backpatch skip
            emit::patch_dword(skip, emit::pos() - (skip + 4));
        } else if tok_is(TOK_DEFAULT) {
            lex::next(); // consume 'default'
            lex::expect(TOK_COLON);

            // Parse default body
            while !tok_is(TOK_CASE)
                && !tok_is(TOK_DEFAULT)
                && !tok_is(TOK_RBRACE)
                && !tok_is(TOK_EOF)
                && !HAD_ERROR
            {
                parse_stmt();
            }
        } else {
            // Unexpected token inside switch body
            parse_stmt();
        }
    }

    // Patch all break jumps to here
    loop_pop();

    lex::expect(TOK_RBRACE);

    // Reclaim temporary local
    LOCAL_OFFSET = old_local_offset;
}

// ---- Statement parsing ----

// Parse one statement
unsafe fn parse_stmt() {
    if HAD_ERROR {
        return;
    }

    // Block
    if tok_is(TOK_LBRACE) {
        parse_block();
        return;
    }

    // return [expr];
    if tok_is(TOK_RETURN) {
        lex::next();
        if !tok_is(TOK_SEMI) {
            parse_expr();
        }
        emit::epilogue();
        lex::expect(TOK_SEMI);
        return;
    }

    // if (expr) stmt [else stmt]
    if tok_is(TOK_IF) {
        lex::next();
        lex::expect(TOK_LPAREN);
        parse_expr();
        lex::expect(TOK_RPAREN);
        emit::cmp_eax_imm(0);
        let false_jump = emit::jcc_placeholder(CC_E);
        parse_stmt();
        if tok_is(TOK_ELSE) {
            lex::next();
            let end_jump = emit::jmp_placeholder();
            emit::patch_dword(false_jump, emit::pos() - (false_jump + 4));
            parse_stmt();
            emit::patch_dword(end_jump, emit::pos() - (end_jump + 4));
        } else {
            emit::patch_dword(false_jump, emit::pos() - (false_jump + 4));
        }
        return;
    }

    // while (expr) stmt
    if tok_is(TOK_WHILE) {
        lex::next();
        let loop_start = emit::pos();
        loop_push(loop_start);
        lex::expect(TOK_LPAREN);
        parse_expr();
        lex::expect(TOK_RPAREN);
        emit::cmp_eax_imm(0);
        let exit_jump = emit::jcc_placeholder(CC_E);
        parse_stmt();
        emit::jmp_rel((loop_start as i32) - (emit::pos() as i32 + 5));
        emit::patch_dword(exit_jump, emit::pos() - (exit_jump + 4));
        loop_pop();
        return;
    }

    // do { body } while (cond);
    if tok_is(TOK_DO) {
        lex::next();
        let loop_start = emit::pos();
        loop_push(loop_start); // temporary; patched below
        parse_stmt();
        let cond_start = emit::pos();
        // Patch continue target to point to condition
        LOOP_STACK[(LOOP_DEPTH - 1) as usize].continue_target = cond_start;
        lex::expect(TOK_WHILE);
        lex::expect(TOK_LPAREN);
        parse_expr();
        lex::expect(TOK_RPAREN);
        emit::cmp_eax_imm(0);
        // jne -> loop_start (loop back if condition is true)
        emit::byte(0x0F);
        emit::byte(0x80 | CC_NE as u8);
        emit::dword((loop_start as i32 - (emit::pos() as i32 + 4)) as u32);
        loop_pop();
        lex::expect(TOK_SEMI);
        return;
    }

    // for (init; cond; update) stmt
    if tok_is(TOK_FOR) {
        lex::next();
        lex::expect(TOK_LPAREN);

        // Init
        if !tok_is(TOK_SEMI) {
            if is_type_token() {
                let mut vt = 0i32;
                let mut vp = 0i32;
                let mut vn = [0u8; 32];
                parse_type(&mut vt, &mut vp);
                string::strncpy(vn.as_mut_ptr(), (*lex::peek()).str_val.as_ptr(), 31);
                vn[31] = 0;
                lex::next();
                LOCAL_OFFSET -= 4;
                sym::add(vn.as_ptr(), SYM_LOCAL, vt, vp, LOCAL_OFFSET, 0);
                if tok_is(TOK_ASSIGN) {
                    lex::next();
                    parse_expr();
                    emit::store_local(LOCAL_OFFSET, REG_EAX);
                }
            } else {
                parse_expr();
            }
        }
        lex::expect(TOK_SEMI);

        // Condition
        let cond_start = emit::pos();
        if !tok_is(TOK_SEMI) {
            parse_expr();
        } else {
            emit::mov_reg_imm(REG_EAX, 1);
        }
        lex::expect(TOK_SEMI);

        emit::cmp_eax_imm(0);
        let body_end_jump = emit::jcc_placeholder(CC_E);

        // Jump over the update code (emitted next)
        let skip_update = emit::jmp_placeholder();
        let update_start = emit::pos();

        // Update
        if !tok_is(TOK_RPAREN) {
            parse_expr();
        }
        lex::expect(TOK_RPAREN);

        // After update, jump back to condition
        emit::jmp_rel((cond_start as i32) - (emit::pos() as i32 + 5));

        // Body starts here
        emit::patch_dword(skip_update, emit::pos() - (skip_update + 4));
        loop_push(update_start);
        parse_stmt();

        // After body, jump to update
        emit::jmp_rel((update_start as i32) - (emit::pos() as i32 + 5));

        // Patch exit
        emit::patch_dword(body_end_jump, emit::pos() - (body_end_jump + 4));
        loop_pop();
        return;
    }

    // switch (expr) { case ...: ... }
    if tok_is(TOK_SWITCH) {
        parse_switch();
        return;
    }

    // break;
    if tok_is(TOK_BREAK) {
        lex::next();
        let patch = emit::jmp_placeholder();
        loop_add_break(patch);
        lex::expect(TOK_SEMI);
        return;
    }

    // continue;
    if tok_is(TOK_CONTINUE) {
        lex::next();
        if LOOP_DEPTH > 0 {
            let target = LOOP_STACK[(LOOP_DEPTH - 1) as usize].continue_target;
            emit::jmp_rel((target as i32) - (emit::pos() as i32 + 5));
        } else {
            cc_error(b"continue outside loop\0");
        }
        lex::expect(TOK_SEMI);
        return;
    }

    // Enum declaration inside block
    if tok_is(TOK_ENUM) {
        parse_enum();
        return;
    }

    // Local variable declaration
    if is_type_token() {
        let mut vtype = 0i32;
        let mut vptr = 0i32;
        let mut local_sname = [0u8; 32];

        parse_type(&mut vtype, &mut vptr);
        string::strncpy(local_sname.as_mut_ptr(), PARSED_STRUCT_NAME.as_ptr(), 31);
        local_sname[31] = 0;

        loop {
            if HAD_ERROR {
                return;
            }
            let mut vname = [0u8; 32];
            string::strncpy(vname.as_mut_ptr(), (*lex::peek()).str_val.as_ptr(), 31);
            vname[31] = 0;
            lex::next();

            // Array: type arr[N]; or type arr[N] = {v1, v2, ...};
            if tok_is(TOK_LBRACKET) {
                lex::next();
                let arr_size = (*lex::peek()).num_val;
                lex::next();
                lex::expect(TOK_RBRACKET);
                let elem_sz = if vtype == TYPE_CHAR { 1 } else { 4 };
                let mut total = arr_size * elem_sz;
                total = (total + 3) & !3; // align to 4
                LOCAL_OFFSET -= total;
                sym::add(vname.as_ptr(), SYM_LOCAL, vtype, 1, LOCAL_OFFSET, 0);
                // Array initializer: = {v1, v2, ...}
                if tok_is(TOK_ASSIGN) {
                    lex::next();
                    if tok_is(TOK_LBRACE) {
                        let mut idx = 0i32;
                        lex::next();
                        while !tok_is(TOK_RBRACE) && !tok_is(TOK_EOF) && !HAD_ERROR {
                            let off = LOCAL_OFFSET + idx * elem_sz;
                            parse_expr();
                            emit::store_local(off, REG_EAX);
                            idx += 1;
                            if tok_is(TOK_COMMA) {
                                lex::next();
                            }
                        }
                        lex::expect(TOK_RBRACE);
                    } else {
                        cc_error(b"expected '{' for array initializer\0");
                    }
                }
            } else if local_sname[0] != 0 && vptr == 0 {
                // Struct value variable: allocate def->size bytes on stack
                let sdef = sym::struct_def_lookup(local_sname.as_ptr());
                let sz = if !sdef.is_null() { (*sdef).size } else { 4 };
                LOCAL_OFFSET -= sz;
                sym::add(vname.as_ptr(), SYM_LOCAL, vtype, 0, LOCAL_OFFSET, 0);
                set_sym_struct(vname.as_ptr(), local_sname.as_ptr());
            } else {
                LOCAL_OFFSET -= 4;
                sym::add(vname.as_ptr(), SYM_LOCAL, vtype, vptr, LOCAL_OFFSET, 0);
                if local_sname[0] != 0 {
                    set_sym_struct(vname.as_ptr(), local_sname.as_ptr());
                }
                if tok_is(TOK_ASSIGN) {
                    lex::next();
                    parse_expr();
                    emit::store_local(LOCAL_OFFSET, REG_EAX);
                }
            }
            if tok_is(TOK_COMMA) {
                lex::next();
                continue;
            }
            break;
        }
        lex::expect(TOK_SEMI);
        return;
    }

    // Empty statement
    if tok_is(TOK_SEMI) {
        lex::next();
        return;
    }

    // Expression statement
    parse_expr();
    lex::expect(TOK_SEMI);
}

// Parse a brace-enclosed block
unsafe fn parse_block() {
    if HAD_ERROR {
        return;
    }
    lex::expect(TOK_LBRACE);
    sym::enter_scope();
    while !tok_is(TOK_RBRACE) && !tok_is(TOK_EOF) && !HAD_ERROR {
        parse_stmt();
    }
    sym::leave_scope();
    lex::expect(TOK_RBRACE);
}

// ---- Top-level ----

fn global_data_base() -> u32 {
    emit::get_base() + emit::CC_CODE_MAX as u32
}

static mut GLOBAL_OFFSET: u32 = 0;

// Parse a function definition
unsafe fn parse_function(fn_type: i32, is_ptr: i32, name: *const u8) {
    let func_start = emit::pos();
    sym::add(name, SYM_FUNC, fn_type, is_ptr, func_start as i32, 0);
    sym::enter_scope();

    // Parameters
    lex::expect(TOK_LPAREN);
    let mut param_count: i32 = 0;

    if !tok_is(TOK_RPAREN) {
        if tok_is(TOK_VOID) {
            lex::next();
            if tok_is(TOK_RPAREN) {
                // void parameter list -- no params
            } else {
                // void *name
                let mut pp = 0i32;
                while tok_is(TOK_STAR) {
                    pp = 1;
                    lex::next();
                }
                if tok_is(TOK_IDENT) {
                    param_count += 1;
                    sym::add(
                        (*lex::peek()).str_val.as_ptr(),
                        SYM_PARAM,
                        TYPE_VOID,
                        pp,
                        8 + (param_count - 1) * 4,
                        0,
                    );
                    lex::next();
                }
                // Continue parsing remaining params after void *
                while tok_is(TOK_COMMA) {
                    let mut pt = 0i32;
                    let mut pp2 = 0i32;
                    let mut pn = [0u8; 32];
                    lex::next();
                    if !parse_type(&mut pt, &mut pp2) {
                        cc_error(b"expected parameter type\0");
                        break;
                    }
                    if tok_is(TOK_IDENT) {
                        string::strncpy(
                            pn.as_mut_ptr(),
                            (*lex::peek()).str_val.as_ptr(),
                            31,
                        );
                        pn[31] = 0;
                        param_count += 1;
                        sym::add(pn.as_ptr(), SYM_PARAM, pt, pp2, 8 + (param_count - 1) * 4, 0);
                        if PARSED_STRUCT_NAME[0] != 0 {
                            set_sym_struct(pn.as_ptr(), PARSED_STRUCT_NAME.as_ptr());
                        }
                        lex::next();
                    }
                }
            }
        } else {
            loop {
                let mut pt = 0i32;
                let mut pp = 0i32;
                let mut pn = [0u8; 32];
                if !parse_type(&mut pt, &mut pp) {
                    cc_error(b"expected parameter type\0");
                    break;
                }
                if tok_is(TOK_IDENT) {
                    string::strncpy(pn.as_mut_ptr(), (*lex::peek()).str_val.as_ptr(), 31);
                    pn[31] = 0;
                    param_count += 1;
                    sym::add(pn.as_ptr(), SYM_PARAM, pt, pp, 8 + (param_count - 1) * 4, 0);
                    if PARSED_STRUCT_NAME[0] != 0 {
                        set_sym_struct(pn.as_ptr(), PARSED_STRUCT_NAME.as_ptr());
                    }
                    lex::next();
                }
                if !tok_is(TOK_COMMA) {
                    break;
                }
                lex::next();
            }
        }
    }

    lex::expect(TOK_RPAREN);

    // Function prototype (declaration without body): skip it
    if tok_is(TOK_SEMI) {
        lex::next();
        sym::leave_scope();
        return;
    }

    // Prologue with placeholder for local frame size
    emit::byte(0x55); // push ebp
    emit::byte(0x89);
    emit::byte(0xE5); // mov ebp, esp
    emit::byte(0x81);
    emit::byte(0xEC); // sub esp, imm32
    let prologue_patch = emit::pos();
    emit::dword(0);

    let saved_local_offset = LOCAL_OFFSET;
    LOCAL_OFFSET = 0;

    // Body
    lex::expect(TOK_LBRACE);
    while !tok_is(TOK_RBRACE) && !tok_is(TOK_EOF) && !HAD_ERROR {
        parse_stmt();
    }
    lex::expect(TOK_RBRACE);

    // Default return 0 + epilogue (for fall-through)
    emit::mov_reg_imm(REG_EAX, 0);
    emit::epilogue();

    // Backpatch local frame size
    let total = (-LOCAL_OFFSET + 15) & !15;
    emit::patch_dword(prologue_patch, total as u32);

    LOCAL_OFFSET = saved_local_offset;
    sym::leave_scope();
}

// Parse a global variable declaration
unsafe fn parse_global_var(var_type: i32, is_ptr: i32, name: *const u8) {
    if tok_is(TOK_LBRACKET) {
        lex::next();
        let arr_size = (*lex::peek()).num_val;
        lex::next();
        lex::expect(TOK_RBRACKET);
        let elem_sz = if var_type == TYPE_CHAR { 1 } else { 4 };
        let mut total = arr_size * elem_sz;
        total = (total + 3) & !3; // align to 4
        let addr = global_data_base() + GLOBAL_OFFSET;
        GLOBAL_OFFSET += total as u32;
        sym::add(name, SYM_GLOBAL, var_type, 1, 0, addr);
        lex::expect(TOK_SEMI);
        return;
    }

    let addr = global_data_base() + GLOBAL_OFFSET;
    GLOBAL_OFFSET += 4;
    sym::add(name, SYM_GLOBAL, var_type, is_ptr, 0, addr);

    if tok_is(TOK_ASSIGN) {
        let mut neg = false;
        lex::next();
        if tok_is(TOK_MINUS) {
            neg = true;
            lex::next();
        }
        if tok_is(TOK_NUM) {
            let val: i32 = if neg {
                -((*lex::peek()).num_val)
            } else {
                (*lex::peek()).num_val
            };
            lex::next();
            if (GLOBAL_INIT_COUNT as usize) < MAX_GLOBAL_INITS {
                GLOBAL_INITS[GLOBAL_INIT_COUNT as usize].addr = addr;
                GLOBAL_INITS[GLOBAL_INIT_COUNT as usize].value = val;
                GLOBAL_INIT_COUNT += 1;
            }
        } else {
            cc_error(b"expected constant initializer\0");
        }
    }
    lex::expect(TOK_SEMI);
}

// ---- Entry point ----

// Parse the entire translation unit
pub unsafe fn parse_program() -> i32 {
    HAD_ERROR = false;
    FIXUP_COUNT = 0;
    STRING_COUNT = 0;
    GLOBAL_INIT_COUNT = 0;
    TYPEDEF_COUNT = 0;

    // Pre-seed typedef table with standard kernel types
    string::strncpy(TYPEDEFS[0].name.as_mut_ptr(), b"uint8\0".as_ptr(), 31);
    TYPEDEFS[0].td_type = TYPE_CHAR;
    TYPEDEFS[0].is_ptr = 0;
    TYPEDEF_COUNT = 1;

    LOOP_DEPTH = 0;
    LOCAL_OFFSET = 0;
    GLOBAL_OFFSET = 0;

    // Entry stub: jmp <init_code> (backpatched after parsing)
    emit::byte(0xE9); // jmp rel32
    let main_call_patch = emit::pos();
    emit::dword(0);

    // Parse all top-level declarations
    while !tok_is(TOK_EOF) && !HAD_ERROR {
        let mut tp = 0i32;
        let mut is_ptr = 0i32;
        let mut name = [0u8; 32];
        let mut sname = [0u8; 32];

        // Typedef at top level
        if tok_is(TOK_TYPEDEF) {
            lex::next(); // consume 'typedef'

            // Handle typedef enum { ... } Name;
            if tok_is(TOK_ENUM) {
                parse_enum();
                // Skip optional typedef name after enum
                if tok_is(TOK_IDENT) {
                    lex::next();
                }
                if tok_is(TOK_SEMI) {
                    lex::next();
                }
                continue;
            }

            // Handle typedef struct Name { ... } alias;
            if tok_is(TOK_STRUCT) {
                lex::next(); // 'struct'
                if tok_is(TOK_IDENT) {
                    lex::next(); // struct name
                }
                if tok_is(TOK_LBRACE) {
                    // Skip struct body
                    let mut depth = 1i32;
                    lex::next();
                    while depth > 0 && !tok_is(TOK_EOF) {
                        if tok_is(TOK_LBRACE) {
                            depth += 1;
                        }
                        if tok_is(TOK_RBRACE) {
                            depth -= 1;
                        }
                        lex::next();
                    }
                }
                // The alias name
                if tok_is(TOK_IDENT) {
                    let mut td_name = [0u8; 32];
                    string::strncpy(
                        td_name.as_mut_ptr(),
                        (*lex::peek()).str_val.as_ptr(),
                        31,
                    );
                    td_name[31] = 0;
                    lex::next();
                    if (TYPEDEF_COUNT as usize) < MAX_TYPEDEFS {
                        string::strncpy(
                            TYPEDEFS[TYPEDEF_COUNT as usize].name.as_mut_ptr(),
                            td_name.as_ptr(),
                            31,
                        );
                        TYPEDEFS[TYPEDEF_COUNT as usize].td_type = TYPE_INT;
                        TYPEDEFS[TYPEDEF_COUNT as usize].is_ptr = 0;
                        TYPEDEF_COUNT += 1;
                    }
                }
                if tok_is(TOK_SEMI) {
                    lex::next();
                }
                continue;
            }

            // Regular typedef: typedef int uint32; etc.
            let mut td_type = 0i32;
            let mut td_ptr = 0i32;
            if !parse_type(&mut td_type, &mut td_ptr) {
                // Skip to semicolon
                while !tok_is(TOK_SEMI) && !tok_is(TOK_EOF) {
                    lex::next();
                }
                if tok_is(TOK_SEMI) {
                    lex::next();
                }
                continue;
            }
            // Check for pointer
            while tok_is(TOK_STAR) {
                td_ptr = 1;
                lex::next();
            }
            if tok_is(TOK_IDENT) {
                let mut td_name = [0u8; 32];
                string::strncpy(
                    td_name.as_mut_ptr(),
                    (*lex::peek()).str_val.as_ptr(),
                    31,
                );
                td_name[31] = 0;
                lex::next();
                if (TYPEDEF_COUNT as usize) < MAX_TYPEDEFS {
                    string::strncpy(
                        TYPEDEFS[TYPEDEF_COUNT as usize].name.as_mut_ptr(),
                        td_name.as_ptr(),
                        31,
                    );
                    TYPEDEFS[TYPEDEF_COUNT as usize].td_type = td_type;
                    TYPEDEFS[TYPEDEF_COUNT as usize].is_ptr = td_ptr;
                    TYPEDEF_COUNT += 1;
                }
            }
            if tok_is(TOK_SEMI) {
                lex::next();
            }
            continue;
        }

        // Enum definition at top level
        if tok_is(TOK_ENUM) {
            parse_enum();
            continue;
        }

        // Struct definition: struct Name { ... };
        if tok_is(TOK_STRUCT) {
            lex::next(); // consume 'struct'
            if !tok_is(TOK_IDENT) {
                cc_error(b"expected struct name\0");
                break;
            }
            // Peek ahead: is this a definition or a variable/function?
            string::strncpy(name.as_mut_ptr(), (*lex::peek()).str_val.as_ptr(), 31);
            name[31] = 0;
            lex::next(); // consume struct name

            if tok_is(TOK_LBRACE) {
                // struct definition
                let def = sym::struct_def_add(name.as_ptr());
                if def.is_null() {
                    break;
                }

                let mut foffset = 0i32;
                lex::expect(TOK_LBRACE);
                while !tok_is(TOK_RBRACE) && !tok_is(TOK_EOF) && !HAD_ERROR {
                    let mut ftype = 0i32;
                    let mut fptr = 0i32;
                    let mut fname = [0u8; 32];

                    if !parse_type(&mut ftype, &mut fptr) {
                        cc_error(b"expected field type\0");
                        break;
                    }
                    if !tok_is(TOK_IDENT) {
                        cc_error(b"expected field name\0");
                        break;
                    }
                    string::strncpy(fname.as_mut_ptr(), (*lex::peek()).str_val.as_ptr(), 31);
                    fname[31] = 0;
                    lex::next();
                    lex::expect(TOK_SEMI);

                    if (*def).field_count >= MAX_STRUCT_FIELDS as i32 {
                        cc_error(b"too many struct fields\0");
                        break;
                    }

                    let fld = &mut (*def).fields[(*def).field_count as usize];
                    (*def).field_count += 1;
                    string::strncpy(fld.name.as_mut_ptr(), fname.as_ptr(), 31);
                    fld.name[31] = 0;
                    fld.field_type = ftype;
                    fld.is_ptr = fptr;
                    fld.offset = foffset;
                    fld.elem_size = 4;
                    foffset += 4;
                }
                (*def).size = foffset;
                if (*def).size % 4 != 0 {
                    (*def).size = ((*def).size + 3) & !3;
                }

                lex::expect(TOK_RBRACE);
                lex::expect(TOK_SEMI);
                continue;
            }

            // struct variable or function: struct Name [*] ident
            tp = TYPE_INT;
            is_ptr = 0;
            string::strncpy(sname.as_mut_ptr(), name.as_ptr(), 31);
            sname[31] = 0;

            while tok_is(TOK_STAR) {
                is_ptr = 1;
                lex::next();
            }

            if !tok_is(TOK_IDENT) {
                cc_error(b"expected identifier after struct type\0");
                break;
            }
            string::strncpy(name.as_mut_ptr(), (*lex::peek()).str_val.as_ptr(), 31);
            name[31] = 0;
            lex::next();

            if tok_is(TOK_LPAREN) {
                // function with struct return/param type
                parse_function(tp, is_ptr, name.as_ptr());
            } else {
                // global struct variable
                if is_ptr != 0 {
                    // struct pointer: 4 bytes
                    let addr = global_data_base() + GLOBAL_OFFSET;
                    GLOBAL_OFFSET += 4;
                    sym::add(name.as_ptr(), SYM_GLOBAL, tp, is_ptr, 0, addr);
                    set_sym_struct(name.as_ptr(), sname.as_ptr());
                } else {
                    // struct value: allocate def->size bytes
                    let gdef = sym::struct_def_lookup(sname.as_ptr());
                    let gsz = if !gdef.is_null() { (*gdef).size } else { 4 };
                    let addr = global_data_base() + GLOBAL_OFFSET;
                    GLOBAL_OFFSET += gsz as u32;
                    sym::add(name.as_ptr(), SYM_GLOBAL, tp, 0, 0, addr);
                    set_sym_struct(name.as_ptr(), sname.as_ptr());
                }
                lex::expect(TOK_SEMI);
            }
            continue;
        }

        if !parse_type(&mut tp, &mut is_ptr) {
            cc_error(b"expected type at top level\0");
            break;
        }
        if !tok_is(TOK_IDENT) {
            cc_error(b"expected identifier\0");
            break;
        }
        string::strncpy(name.as_mut_ptr(), (*lex::peek()).str_val.as_ptr(), 31);
        name[31] = 0;
        lex::next();

        if tok_is(TOK_LPAREN) {
            parse_function(tp, is_ptr, name.as_ptr());
        } else {
            parse_global_var(tp, is_ptr, name.as_ptr());
        }
    }

    if HAD_ERROR || lex::had_error() {
        return -1;
    }

    // Resolve forward references
    for i in 0..FIXUP_COUNT as usize {
        let s = sym::lookup(FIXUPS[i].name.as_ptr());
        if s.is_null() {
            vga::puts(b"cc: undefined function: ");
            vga::puts(&FIXUPS[i].name);
            vga::putchar(b'\n');
            return -1;
        }
        let target = (*s).offset as u32;
        let patch = FIXUPS[i].patch_pos;
        emit::patch_dword(patch, (target as i32 - (patch as i32 + 4)) as u32);
    }

    // Append string literals after code
    for i in 0..STRING_COUNT as usize {
        let addr = emit::get_base() + emit::pos();
        for j in 0..STRINGS[i].len as usize {
            emit::byte(STRINGS[i].text[j]);
        }
        emit::byte(0);
        emit::patch_dword(STRINGS[i].patch_pos, addr);
    }

    // Emit init code: store global initializers, then call main, ret
    {
        let init_pos = emit::pos();

        // Emit stores for all recorded global initializers
        for i in 0..GLOBAL_INIT_COUNT as usize {
            emit::mov_reg_imm(REG_EAX, GLOBAL_INITS[i].value as u32);
            emit::store_global(GLOBAL_INITS[i].addr, REG_EAX);
        }

        // call main
        let main_sym = sym::lookup(b"main\0".as_ptr());
        if main_sym.is_null() {
            cc_error(b"undefined reference to 'main'\0");
            return -1;
        }
        emit::byte(0xE8);
        let call_patch = emit::pos();
        emit::dword(0);
        emit::ret();

        // Backpatch call main
        let target = (*main_sym).offset as u32;
        emit::patch_dword(call_patch, (target as i32 - (call_patch as i32 + 4)) as u32);

        // Backpatch entry jmp to init_pos
        emit::patch_dword(
            main_call_patch,
            (init_pos as i32 - (main_call_patch as i32 + 4)) as u32,
        );
    }

    0
}
