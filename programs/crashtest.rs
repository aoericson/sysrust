// crashtest.rs -- deliberately triggers a page fault.
//
// Dereferences a null pointer, which causes a Page Fault (vector 14).
// With crash recovery, the OS should print an error and return to the
// shell instead of halting.
fn main() -> i32 {
    let mut p: *mut i32;
    p = 0 as *mut i32;
    *p = 42;
    return 0;
}
