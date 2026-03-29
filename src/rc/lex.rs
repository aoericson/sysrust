// rc/lex.rs -- Lexer (tokenizer) for the Rust-syntax compiler.
//
// Breaks source text into tokens: keywords, identifiers, numbers,
// strings, operators, and punctuation.

use crate::string;
use crate::vga;

// Token types
pub const TOK_EOF: i32 = 0;
// Literals
pub const TOK_NUM: i32 = 1;
pub const TOK_STR: i32 = 2;
pub const TOK_IDENT: i32 = 3;
// Keywords
pub const TOK_I32: i32 = 4;         // was INT
pub const TOK_U8: i32 = 5;          // was CHAR
pub const TOK_VOID: i32 = 6;
pub const TOK_IF: i32 = 7;
pub const TOK_ELSE: i32 = 8;
pub const TOK_WHILE: i32 = 9;
pub const TOK_FOR: i32 = 10;
pub const TOK_LOOP: i32 = 11;       // was DO
pub const TOK_RETURN: i32 = 12;
pub const TOK_BREAK: i32 = 13;
pub const TOK_CONTINUE: i32 = 14;
pub const TOK_STRUCT: i32 = 15;
pub const TOK_ENUM: i32 = 16;
pub const TOK_MATCH: i32 = 17;      // was SWITCH
// 18 unused (was CASE)
pub const TOK_UNDERSCORE_PAT: i32 = 19; // was DEFAULT
pub const TOK_STATIC: i32 = 20;
pub const TOK_CONST: i32 = 21;
// 22 unused (was EXTERN)
pub const TOK_TYPE: i32 = 23;       // was TYPEDEF
pub const TOK_U32: i32 = 24;        // was UNSIGNED
pub const TOK_USIZE: i32 = 25;      // was SIGNED
pub const TOK_BOOL: i32 = 26;       // was SHORT
pub const TOK_ISIZE: i32 = 27;      // was LONG
// Single-char operators
pub const TOK_PLUS: i32 = 28;
pub const TOK_MINUS: i32 = 29;
pub const TOK_STAR: i32 = 30;
pub const TOK_SLASH: i32 = 31;
pub const TOK_PERCENT: i32 = 32;
pub const TOK_AMP: i32 = 33;
pub const TOK_PIPE: i32 = 34;
pub const TOK_CARET: i32 = 35;
// 36 unused (was TILDE -- removed, ! is bitwise NOT in Rust)
pub const TOK_BANG: i32 = 37;
pub const TOK_ASSIGN: i32 = 38;
pub const TOK_LT: i32 = 39;
pub const TOK_GT: i32 = 40;
// Multi-char operators
pub const TOK_EQ: i32 = 41;
pub const TOK_NEQ: i32 = 42;
pub const TOK_LTE: i32 = 43;
pub const TOK_GTE: i32 = 44;
pub const TOK_AND: i32 = 45;
pub const TOK_OR: i32 = 46;
pub const TOK_LSHIFT: i32 = 47;
pub const TOK_RSHIFT: i32 = 48;
pub const TOK_PLUSEQ: i32 = 49;
pub const TOK_MINUSEQ: i32 = 50;
pub const TOK_STAREQ: i32 = 51;
pub const TOK_SLASHEQ: i32 = 52;
// 53-54 unused (was INC DEC -- removed, use += 1)
// Punctuation
pub const TOK_LPAREN: i32 = 55;
pub const TOK_RPAREN: i32 = 56;
pub const TOK_LBRACE: i32 = 57;
pub const TOK_RBRACE: i32 = 58;
pub const TOK_LBRACKET: i32 = 59;
pub const TOK_RBRACKET: i32 = 60;
pub const TOK_SEMI: i32 = 61;
pub const TOK_COMMA: i32 = 62;
// 63 unused (was QUESTION -- removed, no ternary)
pub const TOK_COLON: i32 = 64;
pub const TOK_DOT: i32 = 65;
pub const TOK_ARROW: i32 = 66;      // -> (kept for fn return type AND struct pointer access)
// New tokens
pub const TOK_FN: i32 = 67;
pub const TOK_LET: i32 = 68;
pub const TOK_MUT: i32 = 69;
pub const TOK_TRUE: i32 = 70;
pub const TOK_FALSE: i32 = 71;
pub const TOK_AS: i32 = 72;
pub const TOK_FAT_ARROW: i32 = 73;  // =>
pub const TOK_DOUBLE_DOT: i32 = 74; // ..
pub const TOK_IN: i32 = 75;
pub const TOK_UNSAFE: i32 = 76;

pub struct Token {
    pub tok_type: i32,
    pub num_val: i32,
    pub str_val: [u8; 128],
    pub line: i32,
}

impl Token {
    const fn new() -> Token {
        Token {
            tok_type: 0,
            num_val: 0,
            str_val: [0u8; 128],
            line: 0,
        }
    }
}

// Source text and scanning state
static mut SRC: *const u8 = core::ptr::null();
static mut SRC_LEN: i32 = 0;
static mut POS: i32 = 0;
static mut CUR_LINE: i32 = 0;

// The current (peeked) token
static mut CUR_TOK: Token = Token::new();

// Error flag
static mut LEX_ERROR_FLAG: i32 = 0;

// ---- Internal helpers ----

// Return current character without advancing
unsafe fn peek_char() -> u8 {
    if POS >= SRC_LEN {
        return 0;
    }
    *SRC.offset(POS as isize)
}

// Return current character and advance
unsafe fn next_char() -> u8 {
    if POS >= SRC_LEN {
        return 0;
    }
    let c = *SRC.offset(POS as isize);
    POS += 1;
    if c == b'\n' {
        CUR_LINE += 1;
    }
    c
}

// Skip whitespace and block/line comments
unsafe fn skip_ws() {
    loop {
        let c = peek_char();
        if string::isspace(c) {
            next_char();
            continue;
        }
        // Block comment
        if c == b'/' && POS + 1 < SRC_LEN && *SRC.offset((POS + 1) as isize) == b'*' {
            next_char();
            next_char();
            loop {
                if POS >= SRC_LEN {
                    return;
                }
                if peek_char() == b'*'
                    && POS + 1 < SRC_LEN
                    && *SRC.offset((POS + 1) as isize) == b'/'
                {
                    next_char();
                    next_char();
                    break;
                }
                next_char();
            }
            continue;
        }
        // Line comment
        if c == b'/' && POS + 1 < SRC_LEN && *SRC.offset((POS + 1) as isize) == b'/' {
            next_char();
            next_char();
            while POS < SRC_LEN && peek_char() != b'\n' {
                next_char();
            }
            continue;
        }
        break;
    }
}

// Query whether any lex error occurred
pub fn had_error() -> bool {
    unsafe { LEX_ERROR_FLAG != 0 }
}

// Print a lex error with line number
unsafe fn lex_error(msg: &[u8]) {
    LEX_ERROR_FLAG = 1;
    vga::puts(b"rc: lex error line ");
    print_int(CUR_LINE);
    vga::puts(b": ");
    vga::puts(msg);
    vga::putchar(b'\n');
}

// Print an integer
unsafe fn print_int(n: i32) {
    let mut buf = [0u8; 12];
    let mut i = 0usize;
    let mut val = n;
    if val == 0 {
        vga::putchar(b'0');
        return;
    }
    while val > 0 {
        buf[i] = b'0' + (val % 10) as u8;
        val /= 10;
        i += 1;
    }
    while i > 0 {
        i -= 1;
        vga::putchar(buf[i]);
    }
}

// Keyword table
struct Keyword {
    word: &'static [u8],
    tok: i32,
}

const KEYWORDS: &[Keyword] = &[
    Keyword { word: b"fn\0", tok: TOK_FN },
    Keyword { word: b"let\0", tok: TOK_LET },
    Keyword { word: b"mut\0", tok: TOK_MUT },
    Keyword { word: b"if\0", tok: TOK_IF },
    Keyword { word: b"else\0", tok: TOK_ELSE },
    Keyword { word: b"while\0", tok: TOK_WHILE },
    Keyword { word: b"for\0", tok: TOK_FOR },
    Keyword { word: b"loop\0", tok: TOK_LOOP },
    Keyword { word: b"return\0", tok: TOK_RETURN },
    Keyword { word: b"break\0", tok: TOK_BREAK },
    Keyword { word: b"continue\0", tok: TOK_CONTINUE },
    Keyword { word: b"struct\0", tok: TOK_STRUCT },
    Keyword { word: b"enum\0", tok: TOK_ENUM },
    Keyword { word: b"match\0", tok: TOK_MATCH },
    Keyword { word: b"static\0", tok: TOK_STATIC },
    Keyword { word: b"const\0", tok: TOK_CONST },
    Keyword { word: b"type\0", tok: TOK_TYPE },
    Keyword { word: b"unsafe\0", tok: TOK_UNSAFE },
    Keyword { word: b"in\0", tok: TOK_IN },
    Keyword { word: b"as\0", tok: TOK_AS },
    Keyword { word: b"i32\0", tok: TOK_I32 },
    Keyword { word: b"u8\0", tok: TOK_U8 },
    Keyword { word: b"u32\0", tok: TOK_U32 },
    Keyword { word: b"bool\0", tok: TOK_BOOL },
    Keyword { word: b"usize\0", tok: TOK_USIZE },
    Keyword { word: b"isize\0", tok: TOK_ISIZE },
    Keyword { word: b"true\0", tok: TOK_TRUE },
    Keyword { word: b"false\0", tok: TOK_FALSE },
    Keyword { word: b"void\0", tok: TOK_VOID },
    Keyword { word: b"_\0", tok: TOK_UNDERSCORE_PAT },
];

// ---- Public API ----

// Initialize the lexer with source text
pub unsafe fn init(source: *const u8, length: i32) {
    SRC = source;
    SRC_LEN = length;
    POS = 0;
    CUR_LINE = 1;
    LEX_ERROR_FLAG = 0;
    string::memset(&mut CUR_TOK as *mut Token as *mut u8, 0, core::mem::size_of::<Token>());
}

// Advance to the next token
pub unsafe fn next() {
    skip_ws();

    CUR_TOK.line = CUR_LINE;
    CUR_TOK.num_val = 0;
    CUR_TOK.str_val[0] = 0;

    if POS >= SRC_LEN {
        CUR_TOK.tok_type = TOK_EOF;
        return;
    }

    let c = peek_char();

    // Character literal: 'x' or '\n' etc
    if c == b'\'' {
        next_char(); // skip opening quote
        let mut ch = next_char();
        let val: i32;
        if ch == b'\\' {
            ch = next_char();
            val = match ch {
                b'n' => b'\n' as i32,
                b't' => b'\t' as i32,
                b'\\' => b'\\' as i32,
                b'\'' => b'\'' as i32,
                b'0' => 0,
                _ => ch as i32,
            };
        } else {
            val = ch as i32;
        }
        if peek_char() == b'\'' {
            next_char(); // skip closing quote
        }
        CUR_TOK.tok_type = TOK_NUM;
        CUR_TOK.num_val = val;
        return;
    }

    // Number literal (decimal or hex)
    if string::isdigit(c) {
        let mut val: i32 = 0;
        if c == b'0'
            && POS + 1 < SRC_LEN
            && (*SRC.offset((POS + 1) as isize) == b'x'
                || *SRC.offset((POS + 1) as isize) == b'X')
        {
            next_char(); // '0'
            next_char(); // 'x'
            while POS < SRC_LEN {
                let h = peek_char();
                if h >= b'0' && h <= b'9' {
                    val = val * 16 + (h - b'0') as i32;
                } else if h >= b'a' && h <= b'f' {
                    val = val * 16 + (h - b'a') as i32 + 10;
                } else if h >= b'A' && h <= b'F' {
                    val = val * 16 + (h - b'A') as i32 + 10;
                } else {
                    break;
                }
                next_char();
            }
        } else {
            while POS < SRC_LEN && string::isdigit(peek_char()) {
                val = val * 10 + (peek_char() - b'0') as i32;
                next_char();
            }
        }
        CUR_TOK.tok_type = TOK_NUM;
        CUR_TOK.num_val = val;
        return;
    }

    // Identifier or keyword
    if string::isalpha(c) || c == b'_' {
        let mut si = 0usize;
        while POS < SRC_LEN && (string::isalnum(peek_char()) || peek_char() == b'_') {
            if si < 127 {
                CUR_TOK.str_val[si] = next_char();
                si += 1;
            } else {
                next_char();
            }
        }
        CUR_TOK.str_val[si] = 0;

        // Check keywords
        for kw in KEYWORDS {
            if string::strcmp(CUR_TOK.str_val.as_ptr(), kw.word.as_ptr()) == 0 {
                CUR_TOK.tok_type = kw.tok;
                return;
            }
        }

        CUR_TOK.tok_type = TOK_IDENT;
        return;
    }

    // String literal
    if c == b'"' {
        let mut si = 0usize;
        next_char(); // skip opening quote
        while POS < SRC_LEN && peek_char() != b'"' {
            let mut sc = next_char();
            if sc == b'\\' {
                sc = next_char();
                sc = match sc {
                    b'n' => b'\n',
                    b't' => b'\t',
                    b'\\' => b'\\',
                    b'"' => b'"',
                    b'0' => 0,
                    _ => sc,
                };
            }
            if si < 127 {
                CUR_TOK.str_val[si] = sc;
                si += 1;
            }
        }
        CUR_TOK.str_val[si] = 0;
        if peek_char() == b'"' {
            next_char(); // skip closing quote
        }
        CUR_TOK.tok_type = TOK_STR;
        return;
    }

    // Operators and punctuation
    next_char(); // consume c

    match c {
        b'+' => {
            if peek_char() == b'=' {
                next_char();
                CUR_TOK.tok_type = TOK_PLUSEQ;
            } else {
                CUR_TOK.tok_type = TOK_PLUS;
            }
        }
        b'-' => {
            if peek_char() == b'>' {
                next_char();
                CUR_TOK.tok_type = TOK_ARROW;
            } else if peek_char() == b'=' {
                next_char();
                CUR_TOK.tok_type = TOK_MINUSEQ;
            } else {
                CUR_TOK.tok_type = TOK_MINUS;
            }
        }
        b'*' => {
            if peek_char() == b'=' {
                next_char();
                CUR_TOK.tok_type = TOK_STAREQ;
            } else {
                CUR_TOK.tok_type = TOK_STAR;
            }
        }
        b'/' => {
            if peek_char() == b'=' {
                next_char();
                CUR_TOK.tok_type = TOK_SLASHEQ;
            } else {
                CUR_TOK.tok_type = TOK_SLASH;
            }
        }
        b'%' => {
            CUR_TOK.tok_type = TOK_PERCENT;
        }
        b'&' => {
            if peek_char() == b'&' {
                next_char();
                CUR_TOK.tok_type = TOK_AND;
            } else {
                CUR_TOK.tok_type = TOK_AMP;
            }
        }
        b'|' => {
            if peek_char() == b'|' {
                next_char();
                CUR_TOK.tok_type = TOK_OR;
            } else {
                CUR_TOK.tok_type = TOK_PIPE;
            }
        }
        b'^' => {
            CUR_TOK.tok_type = TOK_CARET;
        }
        b'!' => {
            if peek_char() == b'=' {
                next_char();
                CUR_TOK.tok_type = TOK_NEQ;
            } else {
                CUR_TOK.tok_type = TOK_BANG;
            }
        }
        b'=' => {
            if peek_char() == b'>' {
                next_char();
                CUR_TOK.tok_type = TOK_FAT_ARROW;
            } else if peek_char() == b'=' {
                next_char();
                CUR_TOK.tok_type = TOK_EQ;
            } else {
                CUR_TOK.tok_type = TOK_ASSIGN;
            }
        }
        b'<' => {
            if peek_char() == b'=' {
                next_char();
                CUR_TOK.tok_type = TOK_LTE;
            } else if peek_char() == b'<' {
                next_char();
                CUR_TOK.tok_type = TOK_LSHIFT;
            } else {
                CUR_TOK.tok_type = TOK_LT;
            }
        }
        b'>' => {
            if peek_char() == b'=' {
                next_char();
                CUR_TOK.tok_type = TOK_GTE;
            } else if peek_char() == b'>' {
                next_char();
                CUR_TOK.tok_type = TOK_RSHIFT;
            } else {
                CUR_TOK.tok_type = TOK_GT;
            }
        }
        b'(' => CUR_TOK.tok_type = TOK_LPAREN,
        b')' => CUR_TOK.tok_type = TOK_RPAREN,
        b'{' => CUR_TOK.tok_type = TOK_LBRACE,
        b'}' => CUR_TOK.tok_type = TOK_RBRACE,
        b'[' => CUR_TOK.tok_type = TOK_LBRACKET,
        b']' => CUR_TOK.tok_type = TOK_RBRACKET,
        b';' => CUR_TOK.tok_type = TOK_SEMI,
        b',' => CUR_TOK.tok_type = TOK_COMMA,
        b':' => CUR_TOK.tok_type = TOK_COLON,
        b'.' => {
            if peek_char() == b'.' {
                next_char();
                CUR_TOK.tok_type = TOK_DOUBLE_DOT;
            } else {
                CUR_TOK.tok_type = TOK_DOT;
            }
        }
        _ => {
            lex_error(b"unexpected character\0");
            CUR_TOK.tok_type = TOK_EOF;
        }
    }
}

// Return a pointer to the current token (without consuming it)
pub unsafe fn peek() -> *mut Token {
    &mut CUR_TOK as *mut Token
}

// Token type name for error messages
fn tok_name(t: i32) -> &'static [u8] {
    match t {
        TOK_SEMI => b"';'\0",
        TOK_COMMA => b"','\0",
        TOK_LPAREN => b"'('\0",
        TOK_RPAREN => b"')'\0",
        TOK_LBRACE => b"'{'\0",
        TOK_RBRACE => b"'}'\0",
        TOK_LBRACKET => b"'['\0",
        TOK_RBRACKET => b"']'\0",
        TOK_IDENT => b"identifier\0",
        TOK_NUM => b"number\0",
        TOK_STR => b"string\0",
        TOK_ASSIGN => b"'='\0",
        TOK_EOF => b"end of file\0",
        _ => b"token\0",
    }
}

// Consume the current token if it matches the expected type, else error
pub unsafe fn expect(expected_type: i32) {
    if CUR_TOK.tok_type != expected_type {
        vga::puts(b"rc: line ");
        print_int(CUR_LINE);
        vga::puts(b": expected ");
        vga::puts(tok_name(expected_type));
        vga::puts(b", got ");
        vga::puts(tok_name(CUR_TOK.tok_type));
        vga::putchar(b'\n');
        LEX_ERROR_FLAG = 1;
    }
    next();
}
