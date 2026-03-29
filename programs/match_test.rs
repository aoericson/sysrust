// match_test.rs -- tests: match (translated from switch.c)

const APPLE: i32 = 0;
const BANANA: i32 = 1;
const CHERRY: i32 = 2;

fn describe(x: i32) -> i32 {
    let mut result: i32;
    result = 0;
    match x {
        1 => {
            result = 10;
        },
        2 => {
            result = 20;
        },
        3 => {
            result = 30;
        },
        _ => {
            result = -1;
        },
    }
    return result;
}

fn main() -> i32 {
    if describe(1) != 10 {
        return 1;
    }
    if describe(2) != 20 {
        return 2;
    }
    if describe(3) != 30 {
        return 3;
    }
    if describe(99) != -1 {
        return 4;
    }

    // Test with enum constants
    match BANANA {
        0 => {
            return 5;
        },
        1 => {
        },
        2 => {
            return 6;
        },
        _ => {
        },
    }

    puts("match_test: PASS\n");
    return 0;
}
