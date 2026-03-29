/* malloc.c -- tests: dynamic memory allocation, pointer arithmetic */

int main() {
    int *buf;
    int i;
    int sum;

    buf = (int *)malloc(40);
    if (buf == 0) return 1;

    for (i = 0; i < 10; i = i + 1) {
        *(buf + i) = i * i;
    }

    sum = 0;
    for (i = 0; i < 10; i = i + 1) {
        sum = sum + *(buf + i);
    }

    free(buf);

    /* 0+1+4+9+16+25+36+49+64+81 = 285 */
    if (sum != 285) return 1;
    puts("malloc: PASS\n");
    return 0;
}
