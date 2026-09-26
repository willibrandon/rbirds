/*
 * The native half of the ABI check for src/platform (docs/DESIGN.md §6).
 *
 * Prints, from the system headers of the machine it is compiled on, one line
 * per fact the Rust bindings depend on: the size, alignment and field offsets
 * of every native structure, the width and signedness of every scalar type,
 * the value of every constant, and whether the header's prototype of every
 * function called matches the prototype the Rust declaration assumes.
 * `rbirds::platform::abi::rust_layout_report()` prints the same lines from the
 * Rust side; tests/abi.rs requires the two reports to be identical.
 *
 * The feature macros are the reference's own (boids.c lines 2-4), so the
 * headers declare what cbirds sees. C11 is needed for _Generic and _Alignof;
 * this is a validation artifact and never part of any build of rbirds.
 *
 *     cc -std=c11 -Wall -Wextra -o abi_probe tools/oracle/abi_probe.c
 *     ./abi_probe            # the layout report
 *     ./abi_probe strerror   # "strerror N <text>" for errno 0..140 and -1
 */
#define _XOPEN_SOURCE 700
#define _DEFAULT_SOURCE
#define _DARWIN_C_SOURCE

#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <termios.h>
#include <time.h>
#include <unistd.h>

#if !defined(__APPLE__) && !defined(__linux__)
#error "the probe knows the prototypes of Darwin and GNU/Linux only"
#endif

#define SIZE(name, type) printf("size %s %zu\n", name, sizeof(type))
#define ALIGN(name, type) printf("align %s %zu\n", name, _Alignof(type))
#define OFFSET(name, type, field) printf("offset %s.%s %zu\n", name, #field, offsetof(type, field))
#define FIELD_SIZE(name, type, field) \
    printf("size %s.%s %zu\n", name, #field, sizeof(((type *)0)->field))
#define SIGNED(name, type) printf("signed %s %d\n", name, ((type)-1 < (type)0) ? 1 : 0)
#define SCALAR(name, type) \
    do {                   \
        SIZE(name, type);  \
        ALIGN(name, type); \
        SIGNED(name, type); \
    } while (0)
#define CONST(name) printf("const %s %lld\n", #name, (long long)(name))
#define UCONST(name) printf("const %s %llu\n", #name, (unsigned long long)(name))
/* A function line is printed with the prototype text only when the header's
 * declaration has exactly that type; otherwise the line says MISMATCH. */
#define FN(name, type, text) \
    printf("fn %s %s\n", #name, _Generic(&name, type: text, default: "MISMATCH"))

static void report(void) {
    /* Scalar types in the bindings' signatures. */
    SCALAR("char", char);
    SCALAR("short", short);
    SCALAR("int", int);
    SCALAR("unsigned int", unsigned int);
    SCALAR("long", long);
    SCALAR("unsigned long", unsigned long);
    SCALAR("size_t", size_t);
    SCALAR("ssize_t", ssize_t);
    SCALAR("pid_t", pid_t);
    SCALAR("time_t", time_t);
    SCALAR("clockid_t", clockid_t);
    SCALAR("nfds_t", nfds_t);
    SCALAR("tcflag_t", tcflag_t);
    SCALAR("cc_t", cc_t);
    SCALAR("speed_t", speed_t);
    SIZE("pointer", void *);
    ALIGN("pointer", void *);

    /* struct termios */
    SIZE("termios", struct termios);
    ALIGN("termios", struct termios);
    OFFSET("termios", struct termios, c_iflag);
    OFFSET("termios", struct termios, c_oflag);
    OFFSET("termios", struct termios, c_cflag);
    OFFSET("termios", struct termios, c_lflag);
#ifdef __linux__
    OFFSET("termios", struct termios, c_line);
#endif
    OFFSET("termios", struct termios, c_cc);
    FIELD_SIZE("termios", struct termios, c_cc);
    OFFSET("termios", struct termios, c_ispeed);
    OFFSET("termios", struct termios, c_ospeed);
    CONST(NCCS);
    CONST(VMIN);
    CONST(VTIME);
    CONST(VSUSP);
    UCONST((cc_t)_POSIX_VDISABLE);
    UCONST(BRKINT);
    UCONST(ICRNL);
    UCONST(INPCK);
    UCONST(ISTRIP);
    UCONST(IXON);
    UCONST(OPOST);
    UCONST(CSIZE);
    UCONST(CS8);
    UCONST(ECHO);
    UCONST(ICANON);
    UCONST(IEXTEN);
    UCONST(ISIG);
    CONST(TCSANOW);
    CONST(TCSAFLUSH);

    /* struct winsize and its requests */
    SIZE("winsize", struct winsize);
    ALIGN("winsize", struct winsize);
    OFFSET("winsize", struct winsize, ws_row);
    OFFSET("winsize", struct winsize, ws_col);
    OFFSET("winsize", struct winsize, ws_xpixel);
    OFFSET("winsize", struct winsize, ws_ypixel);
    UCONST((unsigned long)TIOCGWINSZ);
    UCONST((unsigned long)TIOCSWINSZ);
#ifdef __linux__
    /* glibc declares ptsname_r only for _GNU_SOURCE, which would also change
     * strerror_r; the PTY support asks the kernel directly, as glibc does. */
    UCONST((unsigned long)TIOCGPTN);
#endif

    /* struct pollfd */
    SIZE("pollfd", struct pollfd);
    ALIGN("pollfd", struct pollfd);
    OFFSET("pollfd", struct pollfd, fd);
    OFFSET("pollfd", struct pollfd, events);
    OFFSET("pollfd", struct pollfd, revents);
    FIELD_SIZE("pollfd", struct pollfd, events);
    CONST(POLLIN);
    CONST(POLLOUT);
    CONST(POLLERR);
    CONST(POLLHUP);
    CONST(POLLNVAL);

    /* struct timespec and the clock */
    SIZE("timespec", struct timespec);
    ALIGN("timespec", struct timespec);
    OFFSET("timespec", struct timespec, tv_sec);
    OFFSET("timespec", struct timespec, tv_nsec);
    FIELD_SIZE("timespec", struct timespec, tv_nsec);
    CONST(CLOCK_MONOTONIC);

    /* struct sigaction, as the sigaction() wrapper takes it */
    SIZE("sigset_t", sigset_t);
    ALIGN("sigset_t", sigset_t);
    SIZE("sigaction", struct sigaction);
    ALIGN("sigaction", struct sigaction);
    OFFSET("sigaction", struct sigaction, sa_handler);
    OFFSET("sigaction", struct sigaction, sa_mask);
    OFFSET("sigaction", struct sigaction, sa_flags);
#ifdef __linux__
    OFFSET("sigaction", struct sigaction, sa_restorer);
#endif
    FIELD_SIZE("sigaction", struct sigaction, sa_flags);
    /* boids.c stores (int)SA_RESETHAND; that int is what reaches sa_flags. */
    CONST((int)SA_RESETHAND);
    CONST((intptr_t)SIG_IGN);
    CONST((intptr_t)SIG_DFL);
    CONST(SIGHUP);
    CONST(SIGINT);
    CONST(SIGQUIT);
    CONST(SIGABRT);
    CONST(SIGBUS);
    CONST(SIGFPE);
    CONST(SIGKILL);
    CONST(SIGSEGV);
    CONST(SIGPIPE);
    CONST(SIGTERM);

    /* errno values and descriptor flags */
    CONST(EINTR);
    CONST(EIO);
    CONST(EBADF);
    CONST(EAGAIN);
    CONST(EWOULDBLOCK);
    CONST(EINVAL);
    CONST(ENOTTY);
    CONST(EPIPE);
    CONST(ERANGE);
    CONST(STDIN_FILENO);
    CONST(STDOUT_FILENO);
    CONST(STDERR_FILENO);
    CONST(F_GETFL);
    CONST(F_SETFL);
    CONST(F_GETFD);
    CONST(F_SETFD);
    CONST(FD_CLOEXEC);
    CONST(O_NONBLOCK);
    CONST(O_RDWR);
    CONST(O_NOCTTY);
    CONST(O_CLOEXEC);

    /* Prototypes. Top-level parameter qualifiers (restrict) and attributes
     * such as noreturn are not part of the function type being compared. */
    FN(read, ssize_t (*)(int, void *, size_t), "ssize_t(int, void *, size_t)");
    FN(write, ssize_t (*)(int, const void *, size_t), "ssize_t(int, const void *, size_t)");
    FN(fcntl, int (*)(int, int, ...), "int(int, int, ...)");
    FN(strtod, double (*)(const char *, char **), "double(const char *, char **)");
    FN(tcgetattr, int (*)(int, struct termios *), "int(int, struct termios *)");
    FN(tcsetattr, int (*)(int, int, const struct termios *),
       "int(int, int, const struct termios *)");
    FN(ioctl, int (*)(int, unsigned long, ...), "int(int, unsigned long, ...)");
    FN(poll, int (*)(struct pollfd *, nfds_t, int), "int(struct pollfd *, nfds_t, int)");
    FN(clock_gettime, int (*)(clockid_t, struct timespec *), "int(clockid_t, struct timespec *)");
    FN(nanosleep, int (*)(const struct timespec *, struct timespec *),
       "int(const struct timespec *, struct timespec *)");
    FN(time, time_t (*)(time_t *), "time_t(time_t *)");
    /* With these feature macros glibc's strerror_r is the XSI one, which it
     * links as __xpg_strerror_r; the Rust declaration names that symbol. */
    FN(strerror_r, int (*)(int, char *, size_t), "int(int, char *, size_t)");
    FN(isatty, int (*)(int), "int(int)");
    FN(_exit, void (*)(int), "void(int)");
    FN(sigaction, int (*)(int, const struct sigaction *, struct sigaction *),
       "int(int, const struct sigaction *, struct sigaction *)");
    FN(sigemptyset, int (*)(sigset_t *), "int(sigset_t *)");
    FN(kill, int (*)(pid_t, int), "int(pid_t, int)");
    FN(posix_openpt, int (*)(int), "int(int)");
    FN(grantpt, int (*)(int), "int(int)");
    FN(unlockpt, int (*)(int), "int(int)");
#ifdef __APPLE__
    FN(ptsname_r, int (*)(int, char *, size_t), "int(int, char *, size_t)");
    FN(__error, int *(*)(void), "int *(void)");
#else
    FN(__errno_location, int *(*)(void), "int *(void)");
#endif
}

/* What strerror() itself says, for the Rust wrapper over strerror_r to be
 * compared with. The process never calls setlocale, like cbirds and rbirds. */
static void strerror_table(void) {
    for (int e = -1; e <= 140; e++) printf("strerror %d %s\n", e, strerror(e));
}

int main(int argc, char **argv) {
    if (argc > 1 && strcmp(argv[1], "strerror") == 0)
        strerror_table();
    else
        report();
    return 0;
}
