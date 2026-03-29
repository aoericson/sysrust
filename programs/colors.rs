// colors.rs -- tests: string pointer traversal, nested loops

fn main() -> i32 {
    let mut msg: *const u8;
    let mut len: i32;
    let mut i: i32;
    let mut j: i32;
    let mut count: i32;

    msg = "hello";
    len = strlen(msg);
    if len != 5 {
        return 1;
    }

    // Count stars in a triangle pattern
    count = 0;
    i = 0;
    while i < 5 {
        j = 0;
        while j <= i {
            count = count + 1;
            j = j + 1;
        }
        i = i + 1;
    }
    // 1+2+3+4+5 = 15
    if count != 15 {
        return 1;
    }

    puts("colors: PASS\n");
    return 0;
}
