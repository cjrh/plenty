# Bounded channel implementation decision

Plenty channels use a fixed-capacity MPMC ring, explicit endpoint sharing, and
fallible construction. Both blocking and nonblocking operations preserve unsent
ownership. This decision builds on established contracts while preserving Plenty's
recoverable allocation policy.

## Contracts borrowed from existing implementations

Rust's [bounded standard channel](https://doc.rust-lang.org/std/sync/mpsc/fn.sync_channel.html)
supplies backpressure and sender cloning. Its receive side is single-consumer.
[Crossbeam](https://docs.rs/crossbeam-channel/latest/crossbeam_channel/) supports
multiple competing receivers and distinguishes an empty/full queue from a
disconnected peer. Buffered messages remain receivable after senders disconnect;
an unsuccessful send returns the message. Plenty adopts these useful behaviors,
without implying broadcast or fairness among waiting threads.

Crossbeam's public `bounded` constructor returns handles directly. The inspected
0.5.17 implementation allocates its array and shared counter internally, without
a recoverable allocation result. Wrapping it would not satisfy Plenty's allocation
contract. No Crossbeam source is copied into the runtime.

## Synchronization choice

One mutex protects queue positions and disconnection state. Separate condition
variables park blocked readers and writers. Predicate loops follow the
[standard condition-variable contract](https://doc.rust-lang.org/std/sync/struct.Condvar.html),
including spurious wakeups. Normal transfers notify one peer; disconnection wakes
all affected peers. User destructors never execute under this lock.

[parking_lot](https://docs.rs/parking_lot/latest/parking_lot/struct.Condvar.html)
offers compact synchronization and useful wakeup policies, including requeueing
waiters during notification. Plenty's currently supported Linux target already
has inline, futex-backed std mutexes and condition variables. Source inspection
and allocation-disabled native tests establish the needed allocation behavior
on that target, so this implementation adds no dependency. This is a target
property to revalidate when porting, not a cross-platform std API guarantee.

## Ownership and costs

The pair record and one core/ring allocation are obtained during construction.
Both endpoint headers live inside the pinned core. Sharing increments the
appropriate atomic count without allocating another endpoint object. Last-drop
disconnects that family; a separate lifetime count keeps the core alive through
queued-message destruction and both endpoint callbacks.

Fixed storage bounds queue memory. Each message is embedded using its native
typed slot size; inline closure captures do not need boxing. Heap-owned messages
transfer their ownership pointers. Receive copies inline payload bytes into
caller storage before freeing a ring slot. These are representation moves, with
one logical owner throughout.

Receiver ownership cycles are rejected by a finite compiler type-graph check:
queued messages must not retain the receiver whose last-drop would drain them.
Ordinary recursive data and sender-based reply paths remain valid. Endpoint
context managers reuse lexical cleanup; placing the receiving guard after its
producer task closes that handle before joining on early exit.

The mutex serializes transfers and provides a straightforward publication point.
There is no lock-free or strict fairness claim. Positive capacities cover the
initial contract; zero-capacity rendezvous is a distinct handshake rather than
a disguised one-slot queue. Scheduling and subsequent extensions remain in the
[backlog](../backlog.md).
