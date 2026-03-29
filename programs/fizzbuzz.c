/* fizzbuzz.c -- tests: while, if/else, modulo, char arithmetic */
int main() {
    int i;
    int count;
    count = 0;
    i = 1;
    while (i <= 15) {
        if (i % 15 == 0) count = count + 1;
        else if (i % 3 == 0) count = count + 1;
        else if (i % 5 == 0) count = count + 1;
        i = i + 1;
    }
    /* 15 numbers: 3,5,6,9,10,12,15 = 7 divisible by 3 or 5 */
    if (count != 7) return 1;
    puts("fizzbuzz: PASS\n");
    return 0;
}
