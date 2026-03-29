// countdown.rs -- tests: while loop (decreasing), function calls, sleep
fn main() -> i32 {
    let mut i: i32;
    puts("Countdown:\n");
    i = 5;
    while i >= 1 {
        putchar('0' + i);
        puts("...\n");
        sleep(50);
        i = i - 1;
    }
    puts("Go!\n");
    return 0;
}
