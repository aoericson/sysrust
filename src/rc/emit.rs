// cc/emit.rs -- x86-32 machine code emitter.
//
// Writes raw x86 instructions into a static code buffer.
// Each helper encodes one logical instruction (or a short sequence).
// The buffer is later written to a VFS file as a flat binary.

use crate::string;

pub const CC_LOAD_BASE: u32 = 0x00A0_0000;
pub const CC_LOAD_BASE2: u32 = 0x00B0_0000;
pub const CC_CODE_MAX: usize = 65536;

// Register constants (matching x86 encoding)
pub const REG_EAX: i32 = 0;
pub const REG_ECX: i32 = 1;
pub const REG_EDX: i32 = 2;
pub const REG_EBX: i32 = 3;
pub const REG_ESP: i32 = 4;
pub const REG_EBP: i32 = 5;
pub const REG_ESI: i32 = 6;
pub const REG_EDI: i32 = 7;

// Condition codes for Jcc / SETcc (low nibble of the 0F 8x / 0F 9x byte)
pub const CC_E: i32 = 0x04;   // equal        (ZF=1)
pub const CC_NE: i32 = 0x05;  // not equal    (ZF=0)
pub const CC_L: i32 = 0x0C;   // less         (SF!=OF)
pub const CC_GE: i32 = 0x0D;  // greater/eq   (SF=OF)
pub const CC_LE: i32 = 0x0E;  // less/eq      (ZF=1 or SF!=OF)
pub const CC_G: i32 = 0x0F;   // greater      (ZF=0 and SF=OF)

// Static code buffer (64 KB, lives in BSS)
static mut CODE_BUF: [u8; CC_CODE_MAX] = [0u8; CC_CODE_MAX];
static mut CODE_POS: u32 = 0;
static mut LOAD_BASE: u32 = CC_LOAD_BASE;
// Set when emit_byte drops a byte due to buffer overflow
static mut EMIT_OVERFLOW_FLAG: i32 = 0;

// Set the target load address for the next compilation
pub fn set_base(base: u32) {
    unsafe { LOAD_BASE = base; }
}

// Get the current load base
pub fn get_base() -> u32 {
    unsafe { LOAD_BASE }
}

// Reset the code buffer to empty
pub fn init() {
    unsafe {
        string::memset(CODE_BUF.as_mut_ptr(), 0, CC_CODE_MAX);
        CODE_POS = 0;
        EMIT_OVERFLOW_FLAG = 0;
    }
}

// Append one byte to the code buffer
pub fn byte(b: u8) {
    unsafe {
        if (CODE_POS as usize) < CC_CODE_MAX {
            CODE_BUF[CODE_POS as usize] = b;
            CODE_POS += 1;
        } else {
            EMIT_OVERFLOW_FLAG = 1;
        }
    }
}

// Append a 32-bit little-endian value
pub fn dword(d: u32) {
    byte((d & 0xFF) as u8);
    byte(((d >> 8) & 0xFF) as u8);
    byte(((d >> 16) & 0xFF) as u8);
    byte(((d >> 24) & 0xFF) as u8);
}

// Return the current write position (offset into code_buf)
pub fn pos() -> u32 {
    unsafe { CODE_POS }
}

// Return a pointer to the code buffer
pub fn code() -> *mut u8 {
    unsafe { CODE_BUF.as_mut_ptr() }
}

// Return the number of bytes emitted so far
pub fn size() -> u32 {
    unsafe { CODE_POS }
}

// Return true if a code buffer overflow occurred since the last init
pub fn had_overflow() -> bool {
    unsafe { EMIT_OVERFLOW_FLAG != 0 }
}

// Overwrite a dword at the given position (for backpatching)
pub fn patch_dword(p: u32, val: u32) {
    unsafe {
        if p + 4 <= CC_CODE_MAX as u32 {
            CODE_BUF[p as usize] = (val & 0xFF) as u8;
            CODE_BUF[(p + 1) as usize] = ((val >> 8) & 0xFF) as u8;
            CODE_BUF[(p + 2) as usize] = ((val >> 16) & 0xFF) as u8;
            CODE_BUF[(p + 3) as usize] = ((val >> 24) & 0xFF) as u8;
        }
    }
}

// push imm32  --  68 imm32
pub fn push_imm(val: i32) {
    byte(0x68);
    dword(val as u32);
}

// push reg  --  50+reg
pub fn push_reg(reg: i32) {
    byte((0x50 + reg) as u8);
}

// pop reg  --  58+reg
pub fn pop_reg(reg: i32) {
    byte((0x58 + reg) as u8);
}

// mov reg, imm32  --  B8+reg imm32
pub fn mov_reg_imm(reg: i32, val: u32) {
    byte((0xB8 + reg) as u8);
    dword(val);
}

// mov dst, src  --  89 ModR/M (mod=11, reg=src, rm=dst)
pub fn mov_reg_reg(dst: i32, src: i32) {
    byte(0x89);
    byte((0xC0 | (src << 3) | dst) as u8);
}

// mov reg, [ebp+offset]  --  8B ModR/M(mod=10, reg, rm=5=ebp) disp32
pub fn load_local(reg: i32, offset: i32) {
    byte(0x8B);
    byte((0x85 | (reg << 3)) as u8);
    dword(offset as u32);
}

// mov [ebp+offset], reg  --  89 ModR/M(mod=10, reg, rm=5=ebp) disp32
pub fn store_local(offset: i32, reg: i32) {
    byte(0x89);
    byte((0x85 | (reg << 3)) as u8);
    dword(offset as u32);
}

// mov reg, [addr]  --  8B ModR/M(mod=00, reg, rm=5) disp32
pub fn load_global(reg: i32, addr: u32) {
    byte(0x8B);
    byte((0x05 | (reg << 3)) as u8);
    dword(addr);
}

// mov [addr], reg  --  89 ModR/M(mod=00, reg, rm=5) disp32
pub fn store_global(addr: u32, reg: i32) {
    byte(0x89);
    byte((0x05 | (reg << 3)) as u8);
    dword(addr);
}

// mov dst, [src]  --  8B ModR/M(mod=00, dst, rm=src)
pub fn load_indirect(dst: i32, src: i32) {
    byte(0x8B);
    if src == REG_EBP {
        // [ebp] needs mod=01 disp8=0 to avoid disp32 encoding
        byte((0x45 | (dst << 3)) as u8);
        byte(0x00);
    } else if src == REG_ESP {
        // [esp] needs SIB byte
        byte((0x04 | (dst << 3)) as u8);
        byte(0x24);
    } else {
        byte(((dst << 3) | src) as u8);
    }
}

// movzx dst, byte [src]  --  0F B6 ModR/M(mod=00, dst, rm=src)
pub fn load_indirect_byte(dst: i32, src: i32) {
    byte(0x0F);
    byte(0xB6);
    if src == REG_EBP {
        byte((0x45 | (dst << 3)) as u8);
        byte(0x00);
    } else if src == REG_ESP {
        byte((0x04 | (dst << 3)) as u8);
        byte(0x24);
    } else {
        byte(((dst << 3) | src) as u8);
    }
}

// mov byte [dst], src_low  --  88 ModR/M(mod=00, src, rm=dst)
pub fn store_indirect_byte(dst: i32, src: i32) {
    byte(0x88);
    if dst == REG_EBP {
        byte((0x45 | (src << 3)) as u8);
        byte(0x00);
    } else if dst == REG_ESP {
        byte((0x04 | (src << 3)) as u8);
        byte(0x24);
    } else {
        byte(((src << 3) | dst) as u8);
    }
}

// mov [dst], src  --  89 ModR/M(mod=00, src, rm=dst)
pub fn store_indirect(dst: i32, src: i32) {
    byte(0x89);
    if dst == REG_EBP {
        byte((0x45 | (src << 3)) as u8);
        byte(0x00);
    } else if dst == REG_ESP {
        byte((0x04 | (src << 3)) as u8);
        byte(0x24);
    } else {
        byte(((src << 3) | dst) as u8);
    }
}

// add dst, src  --  01 ModR/M(mod=11, src, dst)
pub fn add(dst: i32, src: i32) {
    byte(0x01);
    byte((0xC0 | (src << 3) | dst) as u8);
}

// sub dst, src  --  29 ModR/M(mod=11, src, dst)
pub fn sub(dst: i32, src: i32) {
    byte(0x29);
    byte((0xC0 | (src << 3) | dst) as u8);
}

// imul dst, src  --  0F AF ModR/M(mod=11, dst, src)
pub fn imul(dst: i32, src: i32) {
    byte(0x0F);
    byte(0xAF);
    byte((0xC0 | (dst << 3) | src) as u8);
}

// cdq; idiv ecx  --  99  F7 F9
pub fn idiv_ecx() {
    byte(0x99);   // cdq: sign-extend eax into edx:eax
    byte(0xF7);
    byte(0xF9);   // idiv ecx
}

// neg reg  --  F7 ModR/M(mod=11, /3, reg)
pub fn neg(reg: i32) {
    byte(0xF7);
    byte((0xD8 | reg) as u8);
}

// not reg  --  F7 ModR/M(mod=11, /2, reg)
pub fn not(reg: i32) {
    byte(0xF7);
    byte((0xD0 | reg) as u8);
}

// and dst, src  --  21 ModR/M(mod=11, src, dst)
pub fn and(dst: i32, src: i32) {
    byte(0x21);
    byte((0xC0 | (src << 3) | dst) as u8);
}

// or dst, src  --  09 ModR/M(mod=11, src, dst)
pub fn or(dst: i32, src: i32) {
    byte(0x09);
    byte((0xC0 | (src << 3) | dst) as u8);
}

// xor dst, src  --  31 ModR/M(mod=11, src, dst)
pub fn xor(dst: i32, src: i32) {
    byte(0x31);
    byte((0xC0 | (src << 3) | dst) as u8);
}

// shl dst, cl  --  D3 ModR/M(mod=11, /4, dst)
pub fn shl(dst: i32) {
    byte(0xD3);
    byte((0xE0 | dst) as u8);
}

// shr dst, cl  --  D3 ModR/M(mod=11, /5, dst)
pub fn shr(dst: i32) {
    byte(0xD3);
    byte((0xE8 | dst) as u8);
}

// cmp a, b  --  39 ModR/M(mod=11, b, a)
pub fn cmp(a: i32, b: i32) {
    byte(0x39);
    byte((0xC0 | (b << 3) | a) as u8);
}

// setcc reg (low byte)  --  0F 90+cc ModR/M(mod=11, /0, reg)
pub fn setcc(cc: i32, reg: i32) {
    byte(0x0F);
    byte((0x90 + cc) as u8);
    byte((0xC0 | reg) as u8);
}

// call absolute address: mov eax, addr; call eax  --  B8 addr  FF D0
pub fn call_abs(addr: u32) {
    mov_reg_imm(REG_EAX, addr);
    byte(0xFF);
    byte(0xD0);
}

// call rel32  --  E8 rel32
pub fn call_rel(offset: i32) {
    byte(0xE8);
    dword(offset as u32);
}

// ret  --  C3
pub fn ret() {
    byte(0xC3);
}

// jmp rel32  --  E9 rel32
pub fn jmp_rel(offset: i32) {
    byte(0xE9);
    dword(offset as u32);
}

// Emit jmp with placeholder displacement; return position of the disp32
pub fn jmp_placeholder() -> u32 {
    byte(0xE9);
    let p = pos();
    dword(0);
    p
}

// Emit jcc with placeholder displacement; return position of the disp32
pub fn jcc_placeholder(cc: i32) -> u32 {
    byte(0x0F);
    byte((0x80 + cc) as u8);
    let p = pos();
    dword(0);
    p
}

// Function prologue:
//   push ebp        -- 55
//   mov ebp, esp    -- 89 E5
//   sub esp, N      -- 81 EC N (imm32)
pub fn prologue(local_size: i32) {
    byte(0x55);              // push ebp
    byte(0x89);
    byte(0xE5);              // mov ebp, esp
    if local_size > 0 {
        byte(0x81);
        byte(0xEC);          // sub esp, imm32
        dword(local_size as u32);
    }
}

// Function epilogue:
//   mov esp, ebp    -- 89 EC
//   pop ebp         -- 5D
//   ret             -- C3
pub fn epilogue() {
    byte(0x89);
    byte(0xEC);              // mov esp, ebp
    byte(0x5D);              // pop ebp
    byte(0xC3);              // ret
}

// add esp, N  --  81 C4 N (imm32)
pub fn add_esp(n: i32) {
    byte(0x81);
    byte(0xC4);
    dword(n as u32);
}

// cmp eax, imm32  --  3D imm32
pub fn cmp_eax_imm(val: i32) {
    byte(0x3D);
    dword(val as u32);
}

// movzx eax, al  --  0F B6 C0
pub fn movzx_eax_al() {
    byte(0x0F);
    byte(0xB6);
    byte(0xC0);
}
