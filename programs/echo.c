/* echo.c -- tests: getchar, putchar, while loop, char comparison */
int main() {
    int c;
    puts("Type text (press ESC to quit):\n");
    while (1) {
        c = getchar();
        if (c == 27) {
            puts("\nBye!\n");
            return 0;
        }
        putchar(c);
    }
    return 0;
}
