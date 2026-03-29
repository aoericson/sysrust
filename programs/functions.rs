// functions.rs -- tests: user-defined functions, parameters, return values

fn square(x: i32) -> i32 {
    return x * x;
}

fn add(a: i32, b: i32) -> i32 {
    return a + b;
}

fn main() -> i32 {
    if square(7) != 49 {
        return 1;
    }
    if add(30, 12) != 42 {
        return 1;
    }
    if square(add(3, 4)) != 49 {
        return 1;
    }
    puts("functions: PASS\n");
    return 0;
}
