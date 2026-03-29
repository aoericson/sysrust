// test.rs -- Self-contained test runner for the sysrust compiler.
//
// This program is compiled and run by the OS itself.
// It compiles each test program, runs it, and checks the return value.
//
// compile() and run_program() are kernel builtins exposed to compiled code.

fn print_int(n: i32) {
    let mut digits: [i32; 10];
    let mut count: i32;
    let mut i: i32;
    count = 0;
    if n == 0 {
        putchar('0');
        return;
    }
    if n < 0 {
        putchar('-');
        n = 0 - n;
    }
    while n > 0 {
        digits[count] = n % 10;
        count = count + 1;
        n = n / 10;
    }
    i = count - 1;
    while i >= 0 {
        putchar('0' + digits[i]);
        i = i - 1;
    }
}

fn run_test(name: *const u8, bin_name: *const u8) -> i32 {
    let mut r: i32;

    r = compile(name);
    if r != 0 {
        puts("FAIL compile ");
        puts(name);
        puts("\n");
        return 1;
    }

    r = run_program(bin_name);
    if r != 0 {
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

fn main() -> i32 {
    let mut passed: i32;
    let mut failed: i32;

    passed = 0;
    failed = 0;

    puts("=== sysrust compiler test suite ===\n");
    puts("=== TEST START ===\n");

    if run_test("hello.rs", "hello.bin") == 0 {
        passed = passed + 1;
    } else {
        failed = failed + 1;
    }

    if run_test("fizzbuzz.rs", "fizzbuzz.bin") == 0 {
        passed = passed + 1;
    } else {
        failed = failed + 1;
    }

    if run_test("functions.rs", "functions.bin") == 0 {
        passed = passed + 1;
    } else {
        failed = failed + 1;
    }

    if run_test("pointers.rs", "pointers.bin") == 0 {
        passed = passed + 1;
    } else {
        failed = failed + 1;
    }

    if run_test("malloc.rs", "malloc.bin") == 0 {
        passed = passed + 1;
    } else {
        failed = failed + 1;
    }

    if run_test("colors.rs", "colors.bin") == 0 {
        passed = passed + 1;
    } else {
        failed = failed + 1;
    }

    if run_test("chars.rs", "chars.bin") == 0 {
        passed = passed + 1;
    } else {
        failed = failed + 1;
    }

    if run_test("globals.rs", "globals.bin") == 0 {
        passed = passed + 1;
    } else {
        failed = failed + 1;
    }

    if run_test("operators.rs", "operators.bin") == 0 {
        passed = passed + 1;
    } else {
        failed = failed + 1;
    }

    if run_test("break_continue.rs", "break_continue.bin") == 0 {
        passed = passed + 1;
    } else {
        failed = failed + 1;
    }

    if run_test("ternary.rs", "ternary.bin") == 0 {
        passed = passed + 1;
    } else {
        failed = failed + 1;
    }

    if run_test("fileio.rs", "fileio.bin") == 0 {
        passed = passed + 1;
    } else {
        failed = failed + 1;
    }

    if run_test("structs.rs", "structs.bin") == 0 {
        passed = passed + 1;
    } else {
        failed = failed + 1;
    }

    if run_test("enums.rs", "enums.bin") == 0 {
        passed = passed + 1;
    } else {
        failed = failed + 1;
    }

    if run_test("match_test.rs", "match_test.bin") == 0 {
        passed = passed + 1;
    } else {
        failed = failed + 1;
    }

    puts("=== SUMMARY: passed=");
    print_int(passed);
    puts(" failed=");
    print_int(failed);
    puts(" ===\n");
    puts("=== TEST END ===\n");

    return failed;
}
