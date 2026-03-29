// pointers.rs -- tests: pointers, address-of, dereference, swap

fn swap(a: *mut i32, b: *mut i32) {
    let mut tmp: i32;
    tmp = *a;
    *a = *b;
    *b = tmp;
}

fn main() -> i32 {
    let mut x: i32;
    let mut y: i32;

    x = 10;
    y = 20;
    swap(&x, &y);

    if x != 20 {
        return 1;
    }
    if y != 10 {
        return 1;
    }
    puts("pointers: PASS\n");
    return 0;
}
