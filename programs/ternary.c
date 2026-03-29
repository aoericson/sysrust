int abs_val(int x) {
    return x >= 0 ? x : 0 - x;
}

int main() {
    int i;
    int sum;
    int arr[3];

    if (abs_val(-5) != 5) return 1;
    if (abs_val(3) != 3) return 1;

    i = 1;
    sum = 0;
    do {
        sum = sum + i;
        i = i + 1;
    } while (i <= 5);
    if (sum != 15) return 1;

    arr[0] = 10;
    arr[1] = 20;
    arr[2] = 30;
    if (arr[0] + arr[1] + arr[2] != 60) return 1;

    puts("ternary: PASS\n");
    return 0;
}
