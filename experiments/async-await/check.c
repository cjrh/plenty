/* Included after generated.c by test_spike.py. */
static void reset(void) {
    assert(live == 0);
    budget = -1;
    attempts = released = 0;
    resource_drops = 0;
}
static void expect(Outcome result, int kind, int64_t value) {
    assert(result.kind == kind && result.value == value);
}

int main(void) {
    reset();
    v_parent unused = v_parent_new(40);
    assert(attempts == 0);
    v_parent_drop(&unused);
    assert(attempts == 0 && released == 0);

    /* Move after suspension, with both parent and child resources live. */
    v_parent parent = v_parent_new(40);
    expect(v_parent_poll(&parent), PENDING, 0);
    assert(live == 2 && attempts == 2);
    v_parent relocated = parent;
    memset(&parent, 0, sizeof parent);
    parent.pc = -1;
    v_parent_drop(&parent);
    expect(v_parent_poll(&relocated), PENDING, 0);
    assert(live == 1);
    expect(v_parent_poll(&relocated), OK, 42);
    assert(live == 0 && released == 2 && attempts == 2);
    assert(resource_drops == 2 && resource_drop_order[0] == 2 && resource_drop_order[1] == 1);
    expect(v_parent_poll(&relocated), ERROR, FINISHED);
    v_parent_drop(&relocated);
    assert(released == 2);

    /* Cancel at every suspension point and before starting. */
    for (int polls = 0; polls < 3; polls++) {
        reset();
        parent = v_parent_new(1);
        for (int i = 0; i < polls; i++) expect(v_parent_poll(&parent), PENDING, 0);
        int acquired = attempts;
        budget = 0;
        v_parent_drop(&parent);
        assert(live == 0 && attempts == acquired && released == acquired);
        if (polls == 1) assert(resource_drop_order[0] == 2 && resource_drop_order[1] == 1);
    }

    reset();
    v_propagate failure = v_propagate_new(3);
    expect(v_propagate_poll(&failure), PENDING, 0);
    expect(v_propagate_poll(&failure), ERROR, 71);
    assert(live == 0 && released == 2);

    /* Every allocation position: task frame, parent resource, child resource. */
    for (int allowed = 0; allowed <= 3; allowed++) {
        reset();
        Scope scope = {0};
        parent = v_parent_new(40);
        v_parent before = parent;
        budget = allowed;
        int slot = submit(&scope, &parent, sizeof parent,
                          v_parent_erased_poll, v_parent_erased_drop);
        if (allowed == 0) {
            assert(slot == ALLOC && memcmp(&parent, &before, sizeof parent) == 0);
            budget = -1; /* The same unstarted owner can be retried. */
            slot = submit(&scope, &parent, sizeof parent,
                          v_parent_erased_poll, v_parent_erased_drop);
        }
        assert(slot == 0 && parent.pc == -1);
        for (int i = 0; i < 3; i++) step(&scope);
        Outcome result = take(&scope, slot);
        if (allowed == 1 || allowed == 2) expect(result, ERROR, ALLOC);
        else expect(result, OK, 42);
        assert(live == 0);
        close_scope(&scope);
    }

    /* Submission failure after suspension must also preserve owned resources. */
    reset();
    Scope scope = {0};
    parent = v_parent_new(40);
    expect(v_parent_poll(&parent), PENDING, 0);
    v_parent before = parent;
    budget = 0;
    assert(submit(&scope, &parent, sizeof parent,
                  v_parent_erased_poll, v_parent_erased_drop) == ALLOC);
    assert(memcmp(&parent, &before, sizeof parent) == 0 && live == 2);
    budget = -1;
    assert(submit(&scope, &parent, sizeof parent,
                  v_parent_erased_poll, v_parent_erased_drop) == 0);
    budget = 0;
    cancel(&scope, 0);
    expect(take(&scope, 0), ERROR, CANCELLED);
    assert(live == 0 && attempts == 4 && released == 3);

    /* Two roots make progress; full capacity returns the third owner intact. */
    reset();
    v_leaf first = v_leaf_new(10), second = v_leaf_new(20), third = v_leaf_new(30);
    assert(submit(&scope, &first, sizeof first,
                  v_leaf_erased_poll, v_leaf_erased_drop) == 0);
    assert(submit(&scope, &second, sizeof second,
                  v_leaf_erased_poll, v_leaf_erased_drop) == 1);
    int allocated = attempts;
    assert(submit(&scope, &third, sizeof third,
                  v_leaf_erased_poll, v_leaf_erased_drop) == FULL);
    assert(third.pc == 0 && attempts == allocated);
    step(&scope);
    assert(scope.slots[0].result.kind == PENDING && scope.slots[1].result.kind == PENDING);
    step(&scope);
    expect(take(&scope, 0), OK, 11);
    expect(take(&scope, 1), OK, 21);
    assert(submit(&scope, &third, sizeof third,
                  v_leaf_erased_poll, v_leaf_erased_drop) == 0);
    step(&scope);
    budget = 0;
    allocated = attempts;
    close_scope(&scope);
    assert(live == 0 && attempts == allocated);

    printf("PASS: lazy start, nested await, relocation, errors, cancellation, allocation rollback, two roots\n");
    printf("frame bytes: leaf=%zu parent=%zu failing=%zu propagate=%zu scope=%zu\n",
           sizeof(v_leaf), sizeof(v_parent), sizeof(v_failing), sizeof(v_propagate), sizeof(Scope));
    return 0;
}
