//! Returning foreign handlers must coexist with checked traps, and concurrent
//! actor initialization must never bypass the heap table's single owner.
#![cfg(target_os = "linux")]
mod support;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Command, Output};
use std::time::Duration;

const FIXTURE: &str = r#"
#define _GNU_SOURCE
#include <errno.h>
#include <pthread.h>
#include <signal.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/syscall.h>
#include <ucontext.h>
#include <unistd.h>

enum { RETURNING=1, IGNORE, INFO, MASK, NODEFER, ONCE, TWICE, DEFAULT_ACTION,
       CONTEXT, CONCURRENT, IGNORE_FAULT, ALTSTACK, SEGV_HOST, ACTOR_RACE, ACTOR_RETRY, LARGE_HANDLER, RESTART, INTERRUPT, NESTED, NESTED_ALT, IGNORE_ONCE };
static int mode;
static atomic_int calls, invalid, actor_maps;
static char host_stack[131072] __attribute__((aligned(16)));
static pthread_barrier_t start;
extern int64_t plenty_actor_heap(int64_t, int64_t);

static __attribute__((noinline)) void large_frame(void) {
    volatile char frame[131072];
    for (size_t i=0; i<sizeof(frame); i+=4096) frame[i]=(char)i;
    if (frame[0] != 0) atomic_store(&invalid,1);
}
static void inspect(int signal, siginfo_t *info, void *raw) {
    if (mode == LARGE_HANDLER) large_frame();
    volatile int canary=1234;
    int ordinal = atomic_fetch_add(&calls, 1);
    if ((mode == NESTED || mode == NESTED_ALT) && ordinal == 0) raise(SIGILL);
    if (canary != 1234) atomic_store(&invalid,1);
    if (mode == NESTED_ALT) {
        stack_t current;
        if (sigaltstack(NULL,&current) || current.ss_sp != host_stack || !(current.ss_flags & SS_ONSTACK)) atomic_store(&invalid,1);
    }
    if (signal != SIGILL) atomic_store(&invalid, 1);
    if (mode == INFO && (!info || !raw || info->si_signo != SIGILL)) atomic_store(&invalid, 1);
    if (mode == MASK || mode == NODEFER) {
        sigset_t blocked;
        sigprocmask(SIG_SETMASK, NULL, &blocked);
        if (!sigismember(&blocked, SIGUSR1)) atomic_store(&invalid, 1);
        if (sigismember(&blocked, SIGILL) != (mode != NODEFER)) atomic_store(&invalid, 1);
        if (errno != EDOM) atomic_store(&invalid, 1);
    }
    if (mode == CONTEXT && info && info->si_code > 0) {
        ucontext_t *context = raw;
#if defined(__aarch64__)
        context->uc_mcontext.pc += 4;
#else
        context->uc_mcontext.gregs[REG_RIP] += 2;
#endif
    }
}
static void plain(int signal) { inspect(signal, NULL, NULL); }
static void check_stack(void) {
    stack_t current;
    if (sigaltstack(NULL, &current) || current.ss_sp != host_stack || (current.ss_flags & SS_DISABLE)) _exit(89);
}
__attribute__((constructor)) static void setup(void) {
    const char *selected = getenv("PLENTY_SIGNAL_MODE");
    const char *names[] = {"return", "ignore", "info", "mask", "nodefer", "once", "twice", "default",
                           "context", "concurrent", "ignore-fault", "altstack", "segv", "actor-race", "actor-retry", "large-handler", "restart", "interrupt", "nested", "nested-alt", "ignore-once"};
    for (int i=0; i<21; i++) if (selected && !strcmp(selected, names[i])) mode=i+1;
    if (mode == ALTSTACK) {
        stack_t stack = {.ss_sp=host_stack, .ss_size=sizeof(host_stack)};
        if (sigaltstack(&stack, NULL) || atexit(check_stack)) _exit(90);
        return;
    }
    struct sigaction action = {0};
    sigemptyset(&action.sa_mask);
    sigaddset(&action.sa_mask, SIGUSR1);
    if (mode == SEGV_HOST) { action.sa_handler=plain; sigaction(SIGSEGV, &action, NULL); return; }
    if (mode == ACTOR_RACE || mode == ACTOR_RETRY) return;
    action.sa_handler=plain;
    if (mode == IGNORE || mode == IGNORE_FAULT || mode == IGNORE_ONCE) action.sa_handler=SIG_IGN;
    if (mode == DEFAULT_ACTION) action.sa_handler=SIG_DFL;
    if (mode == INFO || mode == CONTEXT) { action.sa_sigaction=inspect; action.sa_flags|=SA_SIGINFO; }
    if (mode == NODEFER || mode == NESTED || mode == NESTED_ALT) action.sa_flags|=SA_NODEFER;
    if (mode == NESTED_ALT) action.sa_flags|=SA_ONSTACK;
    if (mode == RESTART) action.sa_flags|=SA_RESTART;
    if (mode == ONCE || mode == TWICE || mode == IGNORE_ONCE) action.sa_flags|=SA_RESETHAND;
    if (sigaction(SIGILL, &action, NULL)) _exit(91);
}

void *mmap(void *address, size_t length, int protection, int flags, int fd, off_t offset) {
    if ((mode == ACTOR_RACE || mode == ACTOR_RETRY) && length == (UINT64_C(1)<<36) && protection == PROT_NONE) {
        int number = atomic_fetch_add(&actor_maps, 1);
        if (mode == ACTOR_RETRY && number == 0) { errno=ENOMEM; return MAP_FAILED; }
        if (mode == ACTOR_RACE) {
            // Hold the first initializer before BASE publication. The old
            // race admits a second reservation; the fixed owner rejects it.
            if (number != 0) _exit(88);
            for (;;) pause();
        }
    }
    return (void *)syscall(SYS_mmap, address, length, protection, flags, fd, offset);
}
static void *initialize(void *unused) {
    (void)unused;
    if (mode == ACTOR_RACE) pthread_barrier_wait(&start);
    return (void *)(intptr_t)plenty_actor_heap(7, 0);
}
static void *raise_many(void *unused) {
    (void)unused;
    for (int i=0; i<100; i++) raise(SIGILL);
    return NULL;
}
struct interrupt_read { pthread_t target; int writer; };
static void *interrupt_read(void *raw) {
    struct interrupt_read *job = raw;
    usleep(10000);
    pthread_kill(job->target, SIGILL);
    usleep(10000);
    char byte='x';
    if (write(job->writer, &byte, 1) != 1) _exit(94);
    return NULL;
}
int32_t exercise(void) {
    if (mode == RESTART || mode == INTERRUPT) {
        int descriptors[2]; pthread_t thread; char byte=0;
        if (pipe(descriptors)) _exit(95);
        struct interrupt_read job={pthread_self(), descriptors[1]};
        if (pthread_create(&thread, NULL, interrupt_read, &job)) _exit(96);
        ssize_t count=read(descriptors[0], &byte, 1);
        int read_errno=errno;
        pthread_join(thread,NULL);
        close(descriptors[0]); close(descriptors[1]);
        return mode == RESTART ? count == 1 && byte == 'x' : count == -1 && read_errno == EINTR;
    }
    if (mode == ACTOR_RACE) {
        pthread_t a,b;
        if (pthread_barrier_init(&start, NULL, 3) || pthread_create(&a, NULL, initialize, NULL) || pthread_create(&b, NULL, initialize, NULL)) _exit(92);
        pthread_barrier_wait(&start);
        pthread_join(a, NULL); pthread_join(b, NULL);
        return -1;
    }
    if (mode == ACTOR_RETRY) {
        if (plenty_actor_heap(5,0) != 0) return -11;
        if (plenty_actor_heap(7,0) != -3) return -1;
        // Empty queries must not acquire ownership after the failed claim.
        if (plenty_actor_heap(5,0) != 0) return -12;
        pthread_t thread; void *result;
        if (pthread_create(&thread, NULL, initialize, NULL) || pthread_join(thread, &result)) return -2;
        return (intptr_t)result == 0;
    }
    if (mode == ALTSTACK) {
        stack_t current;
        return !sigaltstack(NULL, &current) && current.ss_sp != host_stack && !(current.ss_flags & SS_DISABLE);
    }
    if (mode == SEGV_HOST) {
        struct sigaction current;
        return !sigaction(SIGSEGV, NULL, &current) && current.sa_handler == plain;
    }
    if (mode == CONTEXT || mode == IGNORE_FAULT) {
#if defined(__aarch64__)
        __asm__ volatile(".inst 0");
#else
        __asm__ volatile("ud2");
#endif
    } else if (mode == NESTED_ALT) {
        // Leave enough space for two kernel contexts even on CI hosts with
        // wide extended-register state, then restore Plenty's active stack.
        stack_t generous={.ss_sp=host_stack, .ss_size=sizeof(host_stack)}, saved;
        if (sigaltstack(&generous,&saved)) _exit(97);
        raise(SIGILL);
        if (sigaltstack(&saved,NULL)) _exit(98);
    } else if (mode == CONCURRENT) {
        pthread_t threads[8];
        for (int i=0; i<8; i++) if (pthread_create(&threads[i], NULL, raise_many, NULL)) _exit(93);
        for (int i=0; i<8; i++) pthread_join(threads[i], NULL);
    } else {
        errno=EDOM;
        raise(SIGILL);
        if (mode == TWICE || mode == IGNORE_ONCE) raise(SIGILL);
    }
    if (atomic_load(&invalid)) return -10;
    return mode == IGNORE || mode == IGNORE_ONCE ? 1 : atomic_load(&calls);
}
"#;

fn build(root: &Path, checked_trap: bool, optimize: bool) -> std::path::PathBuf {
    let source = root.join("fixture.c");
    let object = root.join("fixture.o");
    std::fs::write(&source, FIXTURE).unwrap();
    support::assert_success(
        Command::new("cc")
            .args([
                "-std=c11", "-Wall", "-Wextra", "-Werror", "-c", "-fPIC", "-pthread",
            ])
            .arg(source)
            .arg("-o")
            .arg(&object)
            .output()
            .unwrap(),
    );
    std::fs::write(
        root.join("native.plentyi"),
        "pub extern def exercise() -> i32 = \"exercise\"\n",
    )
    .unwrap();
    let main = root.join("main.plenty");
    let check = if checked_trap {
        "    print(add(127i8))?\n"
    } else {
        ""
    };
    std::fs::write(&main, format!("import native\ndef add(value: i8) -> i8:\n    value + 1i8\ndef main() -> Result[(), IoError]:\n    print(native.exercise())?\n{check}    Ok(())\n")).unwrap();
    let executable = root.join("program");
    let options = plenty::CompileOptions {
        optimize,
        link_args: vec![object.into_os_string()],
        ..Default::default()
    };
    plenty::compile_file_to_executable_with_options(&main, &executable, None, &options).unwrap();
    executable
}

fn run(executable: &Path, mode: &str) -> Output {
    support::output_within(
        Command::new("sh")
            .args(["-c", "ulimit -c 0; exec \"$1\"", "sh"])
            .arg(executable)
            .env("PLENTY_SIGNAL_MODE", mode),
        b"",
        Duration::from_secs(20),
    )
}

#[test]
fn previous_handlers_keep_later_checked_traps_and_native_contexts() {
    for optimize in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let program = build(root.path(), true, optimize);
        for (mode, count) in [
            ("return", 1),
            ("ignore", 1),
            ("ignore-once", 1),
            ("info", 1),
            ("mask", 1),
            ("nodefer", 1),
            ("once", 1),
            ("context", 1),
            ("concurrent", 800),
            ("large-handler", 1),
            ("restart", 1),
            ("interrupt", 1),
            ("nested", 2),
            ("nested-alt", 2),
        ] {
            let output = run(&program, mode);
            assert_eq!(output.status.code(), Some(1), "{mode}: {output:?}");
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                format!("{count}\n"),
                "{mode}"
            );
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("integer overflow"),
                "{mode}: {output:?}"
            );
        }
    }
}

#[test]
fn default_reset_once_and_unhandled_instruction_faults_remain_fatal() {
    let root = tempfile::tempdir().unwrap();
    let program = build(root.path(), true, false);
    for mode in ["default", "twice", "ignore-fault"] {
        let output = run(&program, mode);
        assert_eq!(output.status.signal(), Some(4), "{mode}: {output:?}");
    }
}

#[test]
fn host_sigsegv_disposition_and_alternate_stack_survive_program_entry() {
    let root = tempfile::tempdir().unwrap();
    let program = build(root.path(), false, false);
    for mode in ["altstack", "segv"] {
        let output = run(&program, mode);
        assert!(output.status.success(), "{mode}: {output:?}");
        assert_eq!(output.stdout, b"1\n", "{mode}");
    }
}

#[test]
fn concurrent_actor_initialization_rejects_second_reservation_and_failure_retries() {
    let root = tempfile::tempdir().unwrap();
    let program = build(root.path(), false, false);
    let raced = run(&program, "actor-race");
    assert_eq!(raced.status.code(), Some(1), "{raced:?}");
    assert!(
        String::from_utf8_lossy(&raced.stderr).contains("actor heaps belong to the thread"),
        "{raced:?}"
    );
    let retried = run(&program, "actor-retry");
    assert!(retried.status.success(), "{retried:?}");
    assert_eq!(retried.stdout, b"1\n");
}
