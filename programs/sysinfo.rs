// sysinfo.rs -- Display basic system information.

fn print_num(n: i32) {
    if n >= 10000 {
        putchar('0' + n / 10000);
        n = n % 10000;
        putchar('0' + n / 1000);
        n = n % 1000;
        putchar('0' + n / 100);
        n = n % 100;
        putchar('0' + n / 10);
        putchar('0' + n % 10);
    } else if n >= 1000 {
        putchar('0' + n / 1000);
        n = n % 1000;
        putchar('0' + n / 100);
        n = n % 100;
        putchar('0' + n / 10);
        putchar('0' + n % 10);
    } else if n >= 100 {
        putchar('0' + n / 100);
        n = n % 100;
        putchar('0' + n / 10);
        putchar('0' + n % 10);
    } else if n >= 10 {
        putchar('0' + n / 10);
        putchar('0' + n % 10);
    } else {
        putchar('0' + n);
    }
}

fn main() -> i32 {
    let mut ticks: i32;
    let mut seconds: i32;
    let mut heap: i32;
    let mut pages: i32;

    clear();
    set_color(14, 0);
    puts("=== sysrust System Info ===\n\n");
    set_color(7, 0);

    ticks = get_ticks();
    seconds = ticks / 100;
    puts("Uptime: ");
    print_num(seconds);
    puts(" seconds (");
    print_num(ticks);
    puts(" ticks)\n");

    heap = heap_free();
    puts("Heap free: ");
    print_num(heap / 1024);
    puts(" KB\n");

    pages = free_pages();
    puts("Physical pages free: ");
    print_num(pages);
    puts(" (");
    print_num(pages * 4 / 1024);
    puts(" MB)\n");

    return 0;
}
