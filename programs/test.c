/* test.c -- Self-contained test runner for the opsys compiler.
 *
 * This program is compiled and run by the OS itself.
 * It compiles each test program, runs it, and checks the return value.
 *
 * Output goes to VGA (and serial via mirror). The autotest harness in
 * kernel.c reads serial for PASS/FAIL results.
 *
 * compile() and run_program() are kernel builtins exposed to compiled code.
 */

void print_int(int n) {
    int digits[10];
    int count;
    int i;
    count = 0;
    if (n == 0) {
        putchar('0');
        return;
    }
    if (n < 0) {
        putchar('-');
        n = 0 - n;
    }
    while (n > 0) {
        digits[count] = n % 10;
        count = count + 1;
        n = n / 10;
    }
    i = count - 1;
    while (i >= 0) {
        putchar('0' + digits[i]);
        i = i - 1;
    }
}

int run_test(char *name, char *bin_name) {
    int r;

    r = compile(name);
    if (r != 0) {
        puts("FAIL compile ");
        puts(name);
        puts("\n");
        return 1;
    }

    r = run_program(bin_name);
    if (r != 0) {
        puts("FAIL run ");
        puts(name);
        puts("\n");
        return 1;
    }

    puts("PASS ");
    puts(name);
    puts("\n");
    return 0;
}

int main() {
    int passed;
    int failed;

    passed = 0;
    failed = 0;

    puts("=== opsys compiler test suite ===\n");
    puts("=== TEST START ===\n");

    if (run_test("hello.c", "hello.bin") == 0) passed = passed + 1;
    else failed = failed + 1;

    if (run_test("fizzbuzz.c", "fizzbuzz.bin") == 0) passed = passed + 1;
    else failed = failed + 1;

    if (run_test("functions.c", "functions.bin") == 0) passed = passed + 1;
    else failed = failed + 1;

    if (run_test("pointers.c", "pointers.bin") == 0) passed = passed + 1;
    else failed = failed + 1;

    if (run_test("malloc.c", "malloc.bin") == 0) passed = passed + 1;
    else failed = failed + 1;

    if (run_test("colors.c", "colors.bin") == 0) passed = passed + 1;
    else failed = failed + 1;

    if (run_test("chars.c", "chars.bin") == 0) passed = passed + 1;
    else failed = failed + 1;

    if (run_test("globals.c", "globals.bin") == 0) passed = passed + 1;
    else failed = failed + 1;

    if (run_test("operators.c", "operators.bin") == 0) passed = passed + 1;
    else failed = failed + 1;

    if (run_test("break_continue.c", "break_continue.bin") == 0) passed = passed + 1;
    else failed = failed + 1;

    if (run_test("ternary.c", "ternary.bin") == 0) passed = passed + 1;
    else failed = failed + 1;

    if (run_test("fileio.c", "fileio.bin") == 0) passed = passed + 1;
    else failed = failed + 1;

    if (run_test("structs.c", "structs.bin") == 0) passed = passed + 1;
    else failed = failed + 1;

    if (run_test("enums.c", "enums.bin") == 0) passed = passed + 1;
    else failed = failed + 1;

    if (run_test("switch.c", "switch.bin") == 0) passed = passed + 1;
    else failed = failed + 1;

    if (run_test("include_test.c", "include_test.bin") == 0) passed = passed + 1;
    else failed = failed + 1;

    puts("=== SUMMARY: passed=");
    print_int(passed);
    puts(" failed=");
    print_int(failed);
    puts(" ===\n");
    puts("=== TEST END ===\n");

    return failed;
}
