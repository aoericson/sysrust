// string.rs -- Freestanding string and memory utilities.
//
// In a freestanding kernel there is no libc, so we provide our own
// implementations of the standard string/memory functions. These behave
// identically to their POSIX counterparts and operate on raw pointers
// (C-style null-terminated strings).

use core::ptr;

/// Return the length of a null-terminated string.
pub unsafe fn strlen(s: *const u8) -> usize {
    let mut len: usize = 0;
    while *s.add(len) != 0 {
        len += 1;
    }
    len
}

/// Compare two null-terminated strings.
/// Returns 0 if equal, <0 if a < b, >0 if a > b.
pub unsafe fn strcmp(mut a: *const u8, mut b: *const u8) -> i32 {
    while *a != 0 && *a == *b {
        a = a.add(1);
        b = b.add(1);
    }
    (*a as i32) - (*b as i32)
}

/// Compare at most `n` characters of two null-terminated strings.
pub unsafe fn strncmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    for i in 0..n {
        if *a.add(i) != *b.add(i) {
            return (*a.add(i) as i32) - (*b.add(i) as i32);
        }
        if *a.add(i) == 0 {
            return 0;
        }
    }
    0
}

/// Copy src to dest, including the null terminator. Returns dest.
pub unsafe fn strcpy(dest: *mut u8, src: *const u8) -> *mut u8 {
    let mut i: usize = 0;
    loop {
        *dest.add(i) = *src.add(i);
        if *src.add(i) == 0 {
            break;
        }
        i += 1;
    }
    dest
}

/// Copy up to `n` chars from src to dest. Null-pad the remainder. Returns dest.
pub unsafe fn strncpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    let mut i: usize = 0;
    while i < n && *src.add(i) != 0 {
        *dest.add(i) = *src.add(i);
        i += 1;
    }
    while i < n {
        *dest.add(i) = 0;
        i += 1;
    }
    dest
}

/// Append src to end of dest. Returns dest.
pub unsafe fn strcat(dest: *mut u8, src: *const u8) -> *mut u8 {
    let mut d = dest;
    while *d != 0 {
        d = d.add(1);
    }
    let mut s = src;
    loop {
        *d = *s;
        if *s == 0 {
            break;
        }
        d = d.add(1);
        s = s.add(1);
    }
    dest
}

/// Find first occurrence of byte `c` in string `s`.
/// Returns a pointer to the match, or null if not found.
pub unsafe fn strchr(s: *const u8, c: u8) -> *const u8 {
    let mut p = s;
    while *p != 0 {
        if *p == c {
            return p;
        }
        p = p.add(1);
    }
    if c == 0 {
        return p;
    }
    ptr::null()
}

/// Copy `n` bytes from src to dest. Regions must not overlap (use memmove).
pub unsafe fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    for i in 0..n {
        *dest.add(i) = *src.add(i);
    }
    dest
}

/// Fill `n` bytes of dest with the value `val`.
pub unsafe fn memset(dest: *mut u8, val: u8, n: usize) -> *mut u8 {
    for i in 0..n {
        *dest.add(i) = val;
    }
    dest
}

/// Copy `n` bytes from src to dest, correctly handling overlapping regions.
/// If dest < src, copy forward; if dest > src, copy backward.
pub unsafe fn memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    if (dest as usize) < (src as usize) {
        for i in 0..n {
            *dest.add(i) = *src.add(i);
        }
    } else {
        let mut i = n;
        while i > 0 {
            i -= 1;
            *dest.add(i) = *src.add(i);
        }
    }
    dest
}

/// Compare `n` bytes of memory. Returns 0 if equal, <0 or >0 otherwise.
pub unsafe fn memcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    for i in 0..n {
        if *a.add(i) != *b.add(i) {
            return (*a.add(i) as i32) - (*b.add(i) as i32);
        }
    }
    0
}

/// Check if a character is a decimal digit ('0'..'9').
pub fn isdigit(c: u8) -> bool {
    c >= b'0' && c <= b'9'
}

/// Check if a character is alphabetic (a-z, A-Z).
pub fn isalpha(c: u8) -> bool {
    (c >= b'a' && c <= b'z') || (c >= b'A' && c <= b'Z')
}

/// Check if a character is alphanumeric.
pub fn isalnum(c: u8) -> bool {
    isalpha(c) || isdigit(c)
}

/// Check if a character is whitespace.
pub fn isspace(c: u8) -> bool {
    c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' || c == 0x0C || c == 0x0B
}

/// Convert a decimal string to integer. Handles optional leading '-'/'+' and whitespace.
pub unsafe fn atoi(mut s: *const u8) -> i32 {
    let mut result: i32 = 0;
    let mut sign: i32 = 1;

    while isspace(*s) {
        s = s.add(1);
    }

    if *s == b'-' {
        sign = -1;
        s = s.add(1);
    } else if *s == b'+' {
        s = s.add(1);
    }

    while isdigit(*s) {
        result = result * 10 + (*s - b'0') as i32;
        s = s.add(1);
    }

    sign * result
}

/// Search for the first occurrence of `needle` in `haystack`.
/// Both must be null-terminated. Returns true if found, false otherwise.
pub unsafe fn strstr_raw(haystack: *const u8, needle: *const u8) -> bool {
    if *needle == 0 {
        return true;
    }
    let nlen = strlen(needle);
    let mut h = haystack;
    while *h != 0 {
        if strncmp(h, needle, nlen) == 0 {
            return true;
        }
        h = h.add(1);
    }
    false
}
