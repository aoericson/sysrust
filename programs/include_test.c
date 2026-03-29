#include "mylib.h"

int main() {
    if (add(3, 4) != 7) return 1;
    if (mul(5, 6) != 30) return 2;
    puts("include: PASS\n");
    return 0;
}
