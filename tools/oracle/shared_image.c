/* Independent POSIX reader for the macOS shared-image lifecycle tests. */
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

int main(int argc, char **argv) {
    if (argc != 4) return 2;
    int fd = shm_open(argv[2], O_RDONLY);
    if (strcmp(argv[1], "missing") == 0) {
        if (fd >= 0) close(fd);
        return fd < 0 && errno == ENOENT ? 0 : 1;
    }
    if (fd < 0) { perror("shm_open"); return 1; }
    size_t length = (size_t)strtoull(argv[3], NULL, 10);
    struct stat st;
    if (fstat(fd, &st) < 0 || (st.st_mode & 0777) != 0600 || st.st_size <= 0 || (size_t)st.st_size < length) return 1;
    void *data = mmap(NULL, (size_t)st.st_size, PROT_READ, MAP_SHARED, fd, 0);
    if (data == MAP_FAILED) return 1;
    /* Darwin rounds the object size to a VM page. Unwritten padding is zero. */
    for (size_t i = length; i < (size_t)st.st_size; i++) if (((unsigned char *)data)[i] != 0) return 1;
    size_t written = fwrite(data, 1, length, stdout);
    munmap(data, (size_t)st.st_size);
    close(fd);
    if (strcmp(argv[1], "consume") == 0 && shm_unlink(argv[2]) < 0) return 1;
    return written == length ? 0 : 1;
}
