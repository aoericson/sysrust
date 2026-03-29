// colorart.rs -- Colorful display demo using set_color.

fn main() -> i32 {
    let mut col: i32;
    let mut color: i32;

    clear();
    set_color(15, 0);
    puts("Color Art Demo\n\n");

    color = 1;
    while color < 16 {
        set_color(color, 0);
        col = 0;
        while col < color * 3 {
            putchar('#');
            col = col + 1;
        }
        putchar('\n');
        color = color + 1;
    }

    set_color(7, 0);
    puts("\nDone!\n");
    return 0;
}
