/* sysinfo.c -- Display basic system information. */

void print_num(int n) {
    if (n >= 10000) { putchar('0' + n / 10000); n = n % 10000; putchar('0' + n / 1000); n = n % 1000; putchar('0' + n / 100); n = n % 100; putchar('0' + n / 10); putchar('0' + n % 10); }
    else if (n >= 1000) { putchar('0' + n / 1000); n = n % 1000; putchar('0' + n / 100); n = n % 100; putchar('0' + n / 10); putchar('0' + n % 10); }
    else if (n >= 100) { putchar('0' + n / 100); n = n % 100; putchar('0' + n / 10); putchar('0' + n % 10); }
    else if (n >= 10) { putchar('0' + n / 10); putchar('0' + n % 10); }
    else { putchar('0' + n); }
}

int main() {
    int ticks;
    int seconds;
    int heap;
    int pages;

    clear();
    set_color(14, 0);  /* yellow on black */
    puts("=== opsys System Info ===\n\n");
    set_color(7, 0);   /* light grey */

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
