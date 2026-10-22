#ifndef ASYNC_SPIKE_RUNTIME_H
#define ASYNC_SPIKE_RUNTIME_H
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

enum { PENDING, OK, ERROR };
enum { ALLOC = -1, FULL = -2, FINISHED = -3, CANCELLED = -4 };
typedef struct { int kind; int64_t value; } Outcome;
static Outcome ok(int64_t value) { return (Outcome){OK, value}; }
static Outcome error(int64_t value) { return (Outcome){ERROR, value}; }
static Outcome pending(void) { return (Outcome){PENDING, 0}; }
/* Explicit wrapping arithmetic avoids relying on C signed-overflow behavior. */
static int64_t add_i64(int64_t a, int64_t b) {
    uint64_t bits = (uint64_t)a + (uint64_t)b;
    int64_t result;
    memcpy(&result, &bits, sizeof result);
    return result;
}
static int64_t sub_i64(int64_t a, int64_t b) {
    uint64_t bits = (uint64_t)a - (uint64_t)b;
    int64_t result;
    memcpy(&result, &bits, sizeof result);
    return result;
}

/* Failure injection covers every generated/runtime heap allocation. */
static int budget = -1, attempts, live, released;
static int resource_drop_order[16], resource_drops;
static void *try_alloc(size_t size) {
    attempts++;
    if (budget == 0) return NULL;
    if (budget > 0) budget--;
    void *result = malloc(size);
    if (result) live++;
    return result;
}
static void release(void **owner) {
    if (*owner) { free(*owner); *owner = NULL; live--; released++; }
}
static void *acquire(void) {
    void *result = try_alloc(16);
    if (result) *(int *)result = attempts;
    return result;
}
static void release_resource(void **owner) {
    if (*owner) {
        assert(resource_drops < 16);
        resource_drop_order[resource_drops++] = *(int *)*owner;
        release(owner);
    }
}

/* A deterministic two-poll stand-in for readiness, not an I/O backend. */
typedef struct { bool waiting; int64_t value; } Tick;
static Tick tick_new(int64_t value) { return (Tick){false, value}; }
static void tick_drop(Tick *tick) { (void)tick; }
static Outcome tick_poll(Tick *tick) {
    if (!tick->waiting) { tick->waiting = true; return pending(); }
    return ok(tick->value);
}

typedef Outcome (*Poll)(void *);
typedef void (*Drop)(void *);
typedef struct {
    void *frame;
    Poll poll;
    Drop drop;
    bool occupied;
    Outcome result;
} Slot;
typedef struct { Slot slots[2]; } Scope;

/* Reserve first: failure leaves every byte and owner in the caller's frame. */
static int submit(Scope *scope, void *source, size_t size, Poll poll, Drop drop) {
    int index = 0;
    while (index < 2 && scope->slots[index].occupied) index++;
    if (index == 2) return FULL;
    void *frame = try_alloc(size);
    if (!frame) return ALLOC;
    memcpy(frame, source, size);
    memset(source, 0, size);
    /* Every generated frame has pc first; moved-from frames cannot run again. */
    *(int *)source = -1;
    scope->slots[index] = (Slot){frame, poll, drop, true, {PENDING, 0}};
    return index;
}
static void step(Scope *scope) {
    for (int i = 0; i < 2; i++) {
        Slot *slot = &scope->slots[i];
        if (slot->frame) {
            slot->result = slot->poll(slot->frame);
            if (slot->result.kind != PENDING) {
                slot->drop(slot->frame);
                release(&slot->frame);
            }
        }
    }
}
static void cancel(Scope *scope, int index) {
    Slot *slot = &scope->slots[index];
    assert(slot->occupied);
    if (slot->frame) {
        slot->drop(slot->frame);
        release(&slot->frame);
        slot->result = error(CANCELLED);
    }
}
static Outcome take(Scope *scope, int index) {
    Slot *slot = &scope->slots[index];
    assert(slot->occupied && slot->result.kind != PENDING);
    Outcome result = slot->result;
    *slot = (Slot){0};
    return result;
}
static void close_scope(Scope *scope) {
    for (int i = 0; i < 2; i++) {
        if (scope->slots[i].occupied) {
            cancel(scope, i);
            (void)take(scope, i);
        }
    }
}
#endif
