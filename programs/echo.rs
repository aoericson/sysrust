// echo.rs -- tests: getchar, putchar, loop, char comparison
fn main() -> i32 {
    let mut c: i32;
    puts("Type text (press ESC to quit):\n");
    loop {
        c = getchar();
        if c == 27 {
            puts("\nBye!\n");
            return 0;
        }
        putchar(c);
    }
    return 0;
}
