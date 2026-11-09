#define _GNU_SOURCE
#include <assert.h>
#include <dlfcn.h>
#include <pthread.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <clap/entry.h>
#include <clap/events.h>

_Static_assert(sizeof(clap_plugin_entry_t) == 40, "entry layout");
_Static_assert(offsetof(clap_plugin_entry_t, init) == 16, "init layout");
_Static_assert(offsetof(clap_plugin_entry_t, deinit) == 24, "deinit layout");
_Static_assert(offsetof(clap_plugin_entry_t, get_factory) == 32, "factory layout");
_Static_assert(sizeof(clap_input_events_t) == 24, "input layout");
_Static_assert(offsetof(clap_input_events_t, size) == 8, "size layout");
_Static_assert(offsetof(clap_input_events_t, get) == 16, "get layout");
_Static_assert(sizeof(clap_output_events_t) == 16, "output layout");
_Static_assert(offsetof(clap_output_events_t, try_push) == 8, "push layout");

/* A private test factory, deliberately not a CLAP audio-plugin factory. */
struct factory {
    uint32_t (*forward)(const clap_input_events_t *, const clap_output_events_t *);
};
struct context {
    const struct factory *factory;
    clap_event_header_t events[3];
    clap_event_header_t copies[3];
    uint32_t accepted;
    uint32_t calls;
};
static _Thread_local struct context *active;
static uint32_t CLAP_ABI count(const clap_input_events_t *list) {
    assert(list->ctx == active);
    active->calls++;
    return 3;
}
static const clap_event_header_t *CLAP_ABI get(const clap_input_events_t *list, uint32_t index) {
    assert(list->ctx == active && index < 3);
    active->calls++;
    return &active->events[index];
}
static bool CLAP_ABI push(const clap_output_events_t *list, const clap_event_header_t *event) {
    assert(list->ctx == active);
    active->calls++;
    if (event->time % 2) return false;
    assert(active->accepted < 3);
    active->copies[active->accepted++] = *event;
    return true;
}
static void *run(void *data) {
    struct context *ctx = data;
    const clap_input_events_t input = { ctx, count, get };
    const clap_output_events_t output = { ctx, push };
    assert(!active);
    active = ctx;
    for (int round = 0; round < 8; ++round) {
        ctx->accepted = 0;
        ctx->calls = 0;
        assert(ctx->factory->forward(&input, &output) == 2);
        assert(ctx->accepted == 2 && ctx->calls == 7);
        assert(!memcmp(&ctx->copies[0], &ctx->events[0], sizeof(clap_event_header_t)));
        assert(!memcmp(&ctx->copies[1], &ctx->events[2], sizeof(clap_event_header_t)));
    }
    active = NULL;
    return NULL;
}
static bool mapped(const char *path) {
    FILE *maps = fopen("/proc/self/maps", "r");
    assert(maps);
    char line[8192];
    bool found = false;
    while (fgets(line, sizeof(line), maps)) {
        if (strstr(line, path)) found = true;
    }
    assert(!ferror(maps));
    assert(!fclose(maps));
    return found;
}
int main(int argc, char **argv) {
    assert(argc == 2);
    for (int cycle = 0; cycle < 6; ++cycle) {
        void *library = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
        assert(library && mapped(argv[1]));
        const clap_plugin_entry_t *entry = dlsym(library, "clap_entry");
        /* Read the object and callbacks before making any Plenty call. */
        assert(entry && entry->clap_version.major == 1);
        assert(entry->clap_version.minor == 2 && entry->clap_version.revision == 10);
        assert(entry->init && entry->deinit && entry->get_factory);
        assert(!entry->init(NULL));
        assert(entry->init(argv[1]));
        assert(entry->init(argv[1])); /* trivial idempotent initialization */
        assert(!entry->get_factory("unknown.factory"));
        const struct factory *factory = entry->get_factory("plenty.abi-test");
        assert(factory && factory->forward);
        struct context a = { .factory = factory };
        struct context b = { .factory = factory };
        /* Header-only events belong to the fixture's private event space. */
        for (uint32_t i = 0; i < 3; ++i) {
            a.events[i] = (clap_event_header_t){ sizeof(clap_event_header_t), 10+i, 0x1234, 1, 0 };
            b.events[i] = (clap_event_header_t){ sizeof(clap_event_header_t), 100+i, 0x1234, 2, 0 };
        }
        pthread_t first, second;
        assert(!pthread_create(&first, NULL, run, &a));
        assert(!pthread_create(&second, NULL, run, &b));
        assert(!pthread_join(first, NULL));
        assert(!pthread_join(second, NULL));
        /* Join ends the old callback context before handoff to another thread. */
        assert(!pthread_create(&first, NULL, run, &a));
        assert(!pthread_join(first, NULL));
        entry->deinit();
        entry->deinit();
        assert(!dlclose(library));
        /* NODELETE or live runtime TLS destructors would make this succeed. */
        assert(!dlopen(argv[1], RTLD_NOW | RTLD_NOLOAD));
        assert(!mapped(argv[1]));
    }
    return 0;
}
