enum fruit { APPLE, BANANA, CHERRY };

int describe(int x) {
    int result;
    result = 0;
    switch (x) {
    case 1:
        result = 10;
        break;
    case 2:
        result = 20;
        break;
    case 3:
        result = 30;
        break;
    default:
        result = -1;
        break;
    }
    return result;
}

int main() {
    if (describe(1) != 10) return 1;
    if (describe(2) != 20) return 2;
    if (describe(3) != 30) return 3;
    if (describe(99) != -1) return 4;

    /* Test with enum constants */
    switch (BANANA) {
    case APPLE:
        return 5;
    case BANANA:
        break;
    case CHERRY:
        return 6;
    }

    puts("switch: PASS\n");
    return 0;
}
