/* countdown.c -- tests: for loop, function calls, sleep */
int main() {
    int i;
    puts("Countdown:\n");
    for (i = 5; i >= 1; i = i - 1) {
        putchar('0' + i);
        puts("...\n");
        sleep(50);
    }
    puts("Go!\n");
    return 0;
}
