// ternary.rs -- tests: ternary replacement (temp variable), do-while as loop, arrays

fn abs_val(x: i32) -> i32 {
    let mut r: i32;
    if x >= 0 {
        r = x;
    } else {
        r = 0 - x;
    }
    return r;
}

fn main() -> i32 {
    let mut i: i32;
    let mut sum: i32;
    let mut arr: [i32; 3];

    if abs_val(-5) != 5 {
        return 1;
    }
    if abs_val(3) != 3 {
        return 1;
    }

    i = 1;
    sum = 0;
    loop {
        sum = sum + i;
        i = i + 1;
        if i > 5 {
            break;
        }
    }
    if sum != 15 {
        return 1;
    }

    arr[0] = 10;
    arr[1] = 20;
    arr[2] = 30;
    if arr[0] + arr[1] + arr[2] != 60 {
        return 1;
    }

    puts("ternary: PASS\n");
    return 0;
}
