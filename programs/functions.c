/* functions.c -- tests: user-defined functions, parameters, return values */

int square(int x) {
    return x * x;
}

int add(int a, int b) {
    return a + b;
}

int main() {
    if (square(7) != 49) return 1;
    if (add(30, 12) != 42) return 1;
    if (square(add(3, 4)) != 49) return 1;
    puts("functions: PASS\n");
    return 0;
}
