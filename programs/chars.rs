// chars.rs -- tests: string length, char access
fn main() -> i32 {
    let mut msg: *const u8;
    let mut len: i32;

    msg = "hello";
    len = strlen(msg);
    if len != 5 {
        return 1;
    }

    // Test char access
    if *(msg + 0) != 'h' {
        return 2;
    }
    if *(msg + 4) != 'o' {
        return 3;
    }

    puts("chars: PASS\n");
    return 0;
}
