/* colorart.c -- Colorful display demo using set_color. */

int main() {
    int col;
    int color;

    clear();
    set_color(15, 0);  /* white */
    puts("Color Art Demo\n\n");

    for (color = 1; color < 16; color = color + 1) {
        set_color(color, 0);
        for (col = 0; col < color * 3; col = col + 1) {
            putchar('#');
        }
        putchar('\n');
    }

    set_color(7, 0);  /* reset to default */
    puts("\nDone!\n");
    return 0;
}
