// break_continue.rs -- tests: break and continue in loops

fn main() -> i32 {
    let mut i: i32;
    let mut sum: i32;

    // break exits loop early
    sum = 0;
    i = 0;
    while i < 10 {
        if i == 5 {
            break;
        }
        sum = sum + i;
        i = i + 1;
    }
    // sum = 0+1+2+3+4 = 10
    if sum != 10 {
        return 1;
    }
    if i != 5 {
        return 2;
    }

    // continue skips current iteration
    sum = 0;
    i = 0;
    while i < 10 {
        i = i + 1;
        if i % 2 == 0 {
            continue;
        }
        sum = sum + i;
    }
    // sum = 1+3+5+7+9 = 25
    if sum != 25 {
        return 3;
    }

    // break in while loop
    sum = 0;
    i = 0;
    while i < 100 {
        if sum >= 15 {
            break;
        }
        sum = sum + i;
        i = i + 1;
    }
    if sum < 15 {
        return 4;
    }

    puts("break_continue: PASS\n");
    return 0;
}
