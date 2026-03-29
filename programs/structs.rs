// structs.rs -- tests: struct definition, field access, pointer access

struct Point {
    x: i32,
    y: i32,
}

fn set_point(p: *mut Point, x: i32, y: i32) {
    p->x = x;
    p->y = y;
}

fn distance_sq(a: *mut Point, b: *mut Point) -> i32 {
    let mut dx: i32;
    let mut dy: i32;
    dx = a->x - b->x;
    dy = a->y - b->y;
    return dx * dx + dy * dy;
}

fn main() -> i32 {
    let mut p1: Point;
    let mut p2: Point;
    let mut d: i32;

    set_point(&p1, 3, 4);
    set_point(&p2, 0, 0);

    if p1.x != 3 {
        return 1;
    }
    if p1.y != 4 {
        return 2;
    }

    d = distance_sq(&p1, &p2);
    if d != 25 {
        return 3;
    }

    puts("structs: PASS\n");
    return 0;
}
