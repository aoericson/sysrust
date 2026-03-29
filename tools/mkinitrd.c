/*
 * mkinitrd.c -- Build an initrd image from a list of files.
 *
 * Usage: mkinitrd <output> <file1> [file2] ...
 *
 * Format: [count:4] { [name:64] [size:4] [data:size] } ...
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>

#define NAME_LEN 64

int main(int argc, char **argv)
{
    FILE *out;
    uint32_t count;
    int i;

    if (argc < 3) {
        fprintf(stderr, "Usage: %s <output> <file1> [file2] ...\n", argv[0]);
        return 1;
    }

    out = fopen(argv[1], "wb");
    if (!out) { perror(argv[1]); return 1; }

    count = (uint32_t)(argc - 2);
    fwrite(&count, 4, 1, out);

    for (i = 2; i < argc; i++) {
        FILE *in;
        char name[NAME_LEN];
        uint32_t size;
        char *buf;
        const char *basename;

        /* Extract basename */
        basename = strrchr(argv[i], '/');
        basename = basename ? basename + 1 : argv[i];

        memset(name, 0, NAME_LEN);
        strncpy(name, basename, NAME_LEN - 1);

        in = fopen(argv[i], "rb");
        if (!in) { perror(argv[i]); fclose(out); return 1; }

        fseek(in, 0, SEEK_END);
        size = (uint32_t)ftell(in);
        fseek(in, 0, SEEK_SET);

        buf = malloc(size);
        fread(buf, 1, size, in);
        fclose(in);

        fwrite(name, 1, NAME_LEN, out);
        fwrite(&size, 4, 1, out);
        fwrite(buf, 1, size, out);
        free(buf);
    }

    fclose(out);
    printf("Created initrd with %u files\n", count);
    return 0;
}
