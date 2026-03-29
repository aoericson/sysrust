/* colors.c -- tests: string pointer traversal, nested loops */

int main() {
    char *msg;
    int len;
    int i;
    int j;
    int count;

    msg = "hello";
    len = strlen(msg);
    if (len != 5) return 1;

    /* Count stars in a triangle pattern */
    count = 0;
    for (i = 0; i < 5; i = i + 1) {
        for (j = 0; j <= i; j = j + 1) {
            count = count + 1;
        }
    }
    /* 1+2+3+4+5 = 15 */
    if (count != 15) return 1;

    puts("colors: PASS\n");
    return 0;
}
