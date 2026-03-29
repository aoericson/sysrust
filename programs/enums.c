enum color { RED, GREEN, BLUE };
enum { FOO = 10, BAR, BAZ };

int main() {
    if (RED != 0) return 1;
    if (GREEN != 1) return 2;
    if (BLUE != 2) return 3;
    if (FOO != 10) return 4;
    if (BAR != 11) return 5;
    if (BAZ != 12) return 6;
    puts("enums: PASS\n");
    return 0;
}
