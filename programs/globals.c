/* globals.c -- tests: global variable initializers and pointer ++/+= scaling */

int g1 = 42;
int g2 = -7;
int g3 = 100;

int main() {
    int arr[4];
    int *p;
    int i;

    /* Test global initializers */
    if (g1 != 42) return 1;
    if (g2 != -7) return 2;
    if (g3 != 100) return 3;

    /* Test pointer ++ scales by element size */
    arr[0] = 10;
    arr[1] = 20;
    arr[2] = 30;
    arr[3] = 40;
    p = arr;
    if (*p != 10) return 4;
    p++;
    if (*p != 20) return 5;
    p++;
    if (*p != 30) return 6;

    /* Test pointer -- scales by element size */
    p--;
    if (*p != 20) return 7;

    /* Test pointer += scales by element size */
    p = arr;
    p += 3;
    if (*p != 40) return 8;

    /* Test pointer -= scales by element size */
    p -= 2;
    if (*p != 20) return 9;

    puts("globals: PASS\n");
    return 0;
}
