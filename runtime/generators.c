// Concrete native resume functions use fixed, owned frame slots.
typedef uint8_t (*PlentyResume)(void *, uint64_t *);
typedef struct PlentyGenerator {
  PlentyObject object;
  PlentyResume resume;
  uint64_t state, running, count;
  const unsigned char *managed;
  uint64_t slots[];
} PlentyGenerator;
_Static_assert(offsetof(PlentyGenerator, slots) == 56, "generator frame ABI");
void plenty_generator_finish(PlentyGenerator *g) {
  for (size_t i = g->count; i > 0; --i) {
    if (g->managed[i - 1]) plenty_release((void *)(uintptr_t)g->slots[i - 1]);
    g->slots[i - 1] = 0;
  }
  g->state = UINT64_MAX;
}
static void generator_destroy(void *value) {
  plenty_generator_finish(value);
  free(value);
}
PlentyGenerator *plenty_generator_new(PlentyResume resume, uint64_t count, const unsigned char *managed) {
  if (count > (SIZE_MAX - sizeof(PlentyGenerator)) / sizeof(uint64_t)) plenty_value_error("generator frame overflow");
  PlentyGenerator *g = plenty_alloc(sizeof(*g) + count * sizeof(uint64_t));
  g->object = (PlentyObject){1, generator_destroy};
  g->resume = resume;
  g->count = count;
  g->managed = managed;
  return g;
}
uint8_t plenty_generator_resume(PlentyGenerator *g, uint64_t *out) {
  if (g->running) plenty_value_error("generator is already running");
  if (g->state == UINT64_MAX) return 0;
  g->running = 1;
  uint8_t ready = g->resume(g, out);
  g->running = 0;
  return ready;
}
