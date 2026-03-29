// malloc.rs -- tests: dynamic memory allocation, pointer arithmetic

fn main() -> i32 {
    let mut buf: *mut i32;
    let mut i: i32;
    let mut sum: i32;

    buf = malloc(40);
    if buf == 0 {
        return 1;
    }

    i = 0;
    while i < 10 {
        *(buf + i) = i * i;
        i = i + 1;
    }

    sum = 0;
    i = 0;
    while i < 10 {
        sum = sum + *(buf + i);
        i = i + 1;
    }

    free(buf);

    // 0+1+4+9+16+25+36+49+64+81 = 285
    if sum != 285 {
        return 1;
    }
    puts("malloc: PASS\n");
    return 0;
}
