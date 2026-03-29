/* fileio.c -- File I/O test: create, write, read back, verify. */

int main() {
    int fd;
    char buf[64];
    int n;

    /* Create a file and write to it */
    create_file("testfile.txt");
    fd = open("testfile.txt");
    if (fd < 0) return 1;
    fwrite(fd, "Hello from fileio!", 18);
    close(fd);

    /* Read it back */
    fd = open("testfile.txt");
    if (fd < 0) return 2;
    n = read(fd, buf, 63);
    close(fd);

    if (n < 18) return 3;
    buf[n] = 0;

    /* Verify content */
    if (strcmp(buf, "Hello from fileio!") != 0) return 4;

    puts("fileio: PASS\n");
    return 0;
}
