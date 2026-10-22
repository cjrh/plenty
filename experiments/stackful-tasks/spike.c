/* Standalone Linux/glibc experiment; not a Plenty runtime implementation. */
#define _GNU_SOURCE
#include <assert.h>
#include <errno.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/mman.h>
#include <ucontext.h>
#include <unistd.h>

enum { CAPACITY = 2, STACK_BYTES = 64 * 1024 };
enum state { READY, SEND_WAIT, RECV_WAIT, DONE };
enum outcome { SUCCESS, CANCELLED, BODY_OOM };
enum admission { ACCEPTED, FULL, UNAVAILABLE };
struct scope;
struct task {
    ucontext_t context;
    void *mapping;
    size_t mapping_bytes;
    int *job;
    enum state state;
    enum outcome outcome;
    bool cancel;
    enum outcome (*body)(int);
};
struct scope {
    ucontext_t context;
    struct task *tasks[CAPACITY];
    struct task *current;
    size_t count;
    int *slot;
    bool full;
    size_t send_parks;
    size_t receive_parks;
};
static struct scope *active;
static size_t attempts, fail_at, heap_live, mappings_live;
static int drops[16], drop_count, sum, starts, unchanged_before_yield;

static bool fail(void) { return ++attempts == fail_at; }
static void *allocate(size_t bytes) {
    if (fail()) return NULL;
    void *p = calloc(1, bytes);
    if (p) ++heap_live;
    return p;
}
static void release(void *p) {
    if (p) { assert(heap_live > 0); --heap_live; free(p); }
}
static void unmap(struct task *t) {
    assert(mappings_live > 0);
    assert(munmap(t->mapping, t->mapping_bytes) == 0);
    --mappings_live;
}
static bool scope_init(struct scope *s) {
    *s = (struct scope){0};
    s->slot = allocate(sizeof(*s->slot));
    return s->slot != NULL;
}
static void park(enum state state) {
    struct task *t = active->current;
    t->state = state;
    assert(swapcontext(&t->context, &active->context) == 0);
}
static void wake(enum state state) {
    for (size_t i = 0; i < active->count; ++i)
        if (active->tasks[i]->state == state) active->tasks[i]->state = READY;
}
static bool send_value(int value) {
    for (;;) {
        if (active->current->cancel) return false;
        if (!active->full) {
            *active->slot = value;
            active->full = true;
            wake(RECV_WAIT);
            return true;
        }
        ++active->send_parks;
        park(SEND_WAIT);
    }
}
static bool receive_value(int *value) {
    for (;;) {
        if (active->current->cancel) return false;
        if (active->full) {
            *value = *active->slot;
            active->full = false;
            wake(SEND_WAIT);
            return true;
        }
        ++active->receive_parks;
        park(RECV_WAIT);
    }
}
static void enter(void) {
    struct task *t = active->current;
    if (t->cancel) t->outcome = CANCELLED;
    else { ++starts; t->outcome = t->body(*t->job); }
    t->state = DONE;
    /* uc_link returns to the scheduler on its own stack. */
}
static bool configure(struct task *t, struct scope *s, size_t page) {
    if (getcontext(&t->context) != 0) return false;
    t->context.uc_stack.ss_sp = (char *)t->mapping + page;
    t->context.uc_stack.ss_size = STACK_BYTES;
    t->context.uc_link = &s->context;
    makecontext(&t->context, enter, 0);
    return true;
}
/* Ownership of *job changes only at the final commit point. */
static enum admission spawn(struct scope *s, int **job,
                            enum outcome (*body)(int)) {
    if (s->count == CAPACITY) return FULL;
    struct task *t = allocate(sizeof(*t));
    if (!t) return UNAVAILABLE;
    long page_size = sysconf(_SC_PAGESIZE);
    if (page_size <= 0 || STACK_BYTES % (size_t)page_size != 0) {
        release(t); return UNAVAILABLE;
    }
    size_t page = (size_t)page_size;
    t->mapping_bytes = STACK_BYTES + 2 * page;
    t->mapping = fail() ? MAP_FAILED :
        mmap(NULL, t->mapping_bytes, PROT_NONE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (t->mapping == MAP_FAILED) { release(t); return UNAVAILABLE; }
    ++mappings_live;
    if (mprotect((char *)t->mapping + page, STACK_BYTES, PROT_READ | PROT_WRITE) != 0
        || !configure(t, s, page)) {
        unmap(t); release(t); return UNAVAILABLE;
    }
    t->job = *job;
    t->body = body;
    s->tasks[s->count++] = t;
    *job = NULL;
    return ACCEPTED;
}
static bool step(struct scope *s) {
    bool ran = false;
    active = s;
    for (size_t i = 0; i < s->count; ++i) {
        struct task *t = s->tasks[i];
        if (t->state != READY) continue;
        s->current = t;
        assert(swapcontext(&s->context, &t->context) == 0);
        ran = true;
    }
    s->current = NULL;
    return ran;
}
static bool complete(struct scope *s) {
    for (size_t i = 0; i < s->count; ++i)
        if (s->tasks[i]->state != DONE) return false;
    return true;
}
static void cancel_all(struct scope *s) {
    for (size_t i = 0; i < s->count; ++i) {
        struct task *t = s->tasks[i];
        t->cancel = true;
        if (t->state != DONE) t->state = READY;
    }
}
static void destroy(struct scope *s) {
    /* A suspended live stack is never freed as a cancellation shortcut. */
    assert(complete(s));
    for (size_t i = 0; i < s->count; ++i) {
        struct task *t = s->tasks[i];
        release(t->job);
        unmap(t);
        release(t);
    }
    release(s->slot);
}
static int *job(int value) {
    int *p = allocate(sizeof(*p)); assert(p); *p = value; return p;
}
static void reset(void) {
    assert(heap_live == 0 && mappings_live == 0);
    attempts = fail_at = 0;
    sum = starts = drop_count = unchanged_before_yield = 0;
}
static enum outcome producer(int count) {
    int local = 17;
    int *borrow = &local;
    for (int i = 1; i <= count; ++i) {
        if (!send_value(i)) return CANCELLED;
        assert(borrow == &local && *borrow == 17);
    }
    return SUCCESS;
}
static enum outcome consumer(int count) {
    for (int i = 0; i < count; ++i) {
        int value;
        if (!receive_value(&value)) return CANCELLED;
        sum += value;
    }
    return SUCCESS;
}
static void drop(int *resource) {
    drops[drop_count++] = *resource;
    release(resource);
}
static void nested_checkpoint(volatile int *loan) {
    park(READY);
    *loan += 1;
}
static enum outcome cleanup_worker(int ignored) {
    (void)ignored;
    int *first = allocate(sizeof(*first));
    if (!first) return BODY_OOM;
    *first = 1;
    int *second = allocate(sizeof(*second));
    if (!second) { drop(first); return BODY_OOM; }
    *second = 2;
    volatile int local = 17;
    nested_checkpoint(&local);
    assert(local == 18);
    enum outcome result = active->current->cancel ? CANCELLED : SUCCESS;
    drop(second); drop(first);
    return result;
}
static enum outcome no_yield(int ignored) {
    (void)ignored;
    int previous = sum;
    /* Models a non-suspending CPU/foreign call: peers get no execution time. */
    for (int i = 0; i < 10000; ++i) assert(sum == previous);
    unchanged_before_yield = 1;
    return SUCCESS;
}
static enum outcome peer_after_no_yield(int ignored) {
    (void)ignored;
    assert(unchanged_before_yield && sum == 0);
    ++sum;
    return SUCCESS;
}
int main(void) {
    struct scope s;
    reset();
    fail_at = 1;
    assert(!scope_init(&s));
    reset();
    for (size_t site = 1; site <= 2; ++site) {
        assert(scope_init(&s));
        int *input = job(4), *original = input;
        fail_at = attempts + site;
        assert(spawn(&s, &input, producer) == UNAVAILABLE);
        assert(input == original && *input == 4 && s.count == 0);
        assert(heap_live == 2 && mappings_live == 0 && starts == 0);
        fail_at = 0;
        assert(spawn(&s, &input, producer) == ACCEPTED && input == NULL);
        cancel_all(&s);
        assert(step(&s) && complete(&s) && starts == 0);
        destroy(&s); reset();
    }
    printf("admission: queue/task/stack failures roll back; retry retains job\n");

    assert(scope_init(&s));
    int *p = job(1000), *c = job(1000), *extra = job(7);
    assert(spawn(&s, &p, producer) == ACCEPTED);
    assert(spawn(&s, &c, consumer) == ACCEPTED);
    size_t before_full = attempts;
    assert(spawn(&s, &extra, producer) == FULL && *extra == 7);
    assert(attempts == before_full);
    release(extra);
    while (!complete(&s)) assert(step(&s));
    assert(sum == 500500 && s.send_parks > 0 && s.receive_parks > 0);
    assert(s.tasks[0]->outcome == SUCCESS && s.tasks[1]->outcome == SUCCESS);
    printf("channel: sum=%d capacity=1 send_parks=%zu receive_parks=%zu\n",
           sum, s.send_parks, s.receive_parks);
    printf("layout: task_record=%zu fixed_stack=%d per_task_mapping=%zu\n",
           sizeof(struct task), STACK_BYTES, s.tasks[0]->mapping_bytes);
    destroy(&s); reset();

    /* Cancel a genuinely parked sender and receiver, and drain their scopes. */
    for (int mode = 0; mode < 2; ++mode) {
        assert(scope_init(&s));
        int *input = job(2);
        assert(spawn(&s, &input, mode ? consumer : producer) == ACCEPTED);
        assert(step(&s) && !complete(&s));
        assert(s.tasks[0]->state == (mode ? RECV_WAIT : SEND_WAIT));
        assert(!step(&s)); /* No ready task: stall detection, not deadlock proof. */
        cancel_all(&s);
        assert(step(&s) && complete(&s));
        assert(s.tasks[0]->outcome == CANCELLED);
        destroy(&s); reset();
    }
    printf("cancellation: parked send/receive resume and return normally\n");

    for (int mode = 0; mode < 4; ++mode) {
        assert(scope_init(&s));
        int *input = job(0);
        assert(spawn(&s, &input, cleanup_worker) == ACCEPTED);
        if (mode >= 2) fail_at = attempts + (size_t)(mode - 1);
        assert(step(&s));
        if (mode < 2) {
            if (mode == 1) cancel_all(&s);
            assert(step(&s));
        }
        assert(complete(&s));
        assert(s.tasks[0]->outcome == (mode == 0 ? SUCCESS : mode == 1 ? CANCELLED : BODY_OOM));
        assert(drop_count == (mode < 2 ? 2 : mode == 2 ? 0 : 1));
        if (mode < 2) assert(drops[0] == 2 && drops[1] == 1);
        if (mode == 3) assert(drops[0] == 1);
        fail_at = 0;
        destroy(&s); reset();
    }
    assert(scope_init(&s));
    int *input = job(0);
    assert(spawn(&s, &input, no_yield) == ACCEPTED);
    int *peer = job(0);
    assert(spawn(&s, &peer, peer_after_no_yield) == ACCEPTED);
    assert(step(&s) && complete(&s) && unchanged_before_yield && sum == 1);
    destroy(&s); reset();
    printf("cleanup: success/cancel/body-OOM release initialized resources once, reverse order\n");
    printf("scheduler: no preemption; all tracked allocations/mappings released\n");
    return 0;
}
