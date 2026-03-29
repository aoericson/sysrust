/* crashtest.c -- deliberately triggers a page fault.
 *
 * Dereferences a null pointer, which causes a Page Fault (vector 14).
 * With crash recovery, the OS should print an error and return to the
 * shell instead of halting.
 */
int main() {
    int *p;
    p = 0;
    *p = 42;  /* page fault: write to address 0 */
    return 0;
}
