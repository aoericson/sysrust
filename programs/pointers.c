/* pointers.c -- tests: pointers, address-of, dereference, swap */

void swap(int *a, int *b) {
    int tmp;
    tmp = *a;
    *a = *b;
    *b = tmp;
}

int main() {
    int x;
    int y;

    x = 10;
    y = 20;
    swap(&x, &y);

    if (x != 20) return 1;
    if (y != 10) return 1;
    puts("pointers: PASS\n");
    return 0;
}
