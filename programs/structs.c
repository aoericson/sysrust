struct Point {
    int x;
    int y;
};

void set_point(struct Point *p, int x, int y) {
    p->x = x;
    p->y = y;
}

int distance_sq(struct Point *a, struct Point *b) {
    int dx;
    int dy;
    dx = a->x - b->x;
    dy = a->y - b->y;
    return dx * dx + dy * dy;
}

int main() {
    struct Point p1;
    struct Point p2;
    int d;

    set_point(&p1, 3, 4);
    set_point(&p2, 0, 0);

    if (p1.x != 3) return 1;
    if (p1.y != 4) return 2;

    d = distance_sq(&p1, &p2);
    if (d != 25) return 3;

    puts("structs: PASS\n");
    return 0;
}
