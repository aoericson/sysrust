// operators.rs -- tests: bitwise, shift, compound assignment, unary minus operators

fn main() -> i32 {
    let mut a: i32;
    let mut b: i32;
    let mut r: i32;

    a = 0xF0;
    b = 0x0F;

    // bitwise AND
    r = a & b;
    if r != 0 {
        return 1;
    }

    // bitwise OR
    r = a | b;
    if r != 0xFF {
        return 2;
    }

    // bitwise XOR
    r = a ^ b;
    if r != 0xFF {
        return 3;
    }

    // bitwise NOT (! is bitwise NOT in this language, !0 == -1)
    r = !0;
    if r != -1 {
        return 4;
    }

    // left shift
    r = 1 << 4;
    if r != 16 {
        return 5;
    }

    // right shift
    r = 256 >> 3;
    if r != 32 {
        return 6;
    }

    // unary minus
    r = -42;
    if r != -42 {
        return 7;
    }
    r = -r;
    if r != 42 {
        return 8;
    }

    // logical AND short-circuit
    r = 1 && 1;
    if r != 1 {
        return 11;
    }
    r = 1 && 0;
    if r != 0 {
        return 12;
    }

    // logical OR short-circuit
    r = 0 || 1;
    if r != 1 {
        return 13;
    }
    r = 0 || 0;
    if r != 0 {
        return 14;
    }

    // compound assignment += -=
    r = 10;
    r += 5;
    if r != 15 {
        return 15;
    }
    r -= 3;
    if r != 12 {
        return 16;
    }

    // increment/decrement via += and -=
    r = 5;
    r += 1;
    if r != 6 {
        return 17;
    }
    r -= 1;
    if r != 5 {
        return 18;
    }

    puts("operators: PASS\n");
    return 0;
}
