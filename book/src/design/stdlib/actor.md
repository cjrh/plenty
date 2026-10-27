# `std.actor`

Supervised actors on one thread. Every actor owns a heap: its handlers
allocate there, and a trap in a handler discards that heap, closes the files
the actor opened, and tells the actor's parent. The rest of the program keeps
running. Messages are copied into the receiver's heap, mailboxes are bounded
and allocated when the actor starts, and a send that cannot be delivered
returns the message.

Supported target: x86-64 Linux. Elsewhere `supported()` is `False` and starting
an actor returns `StartError.Unsupported`.

A boss starts two workers. One answers; the other has a bug and traps. The boss
receives the answer and both exits:

```plenty
from std.actor import Address, Context, Exit, Mail, heap_bytes_used, heaps_live, stage
from std.ready import WaitError
from std.sync import CancellationToken
from std.time import deadline_after, monotonic_now, seconds

type Fault = AllocError | IoError

pub enum Job:
    Square(i64)
    Answer(i64)

pub class Boss:
    pub me: Option[Address]
    pub ended: i64

pub class Worker:
    pub boss: Address

pub enum Node:
    Leading(Boss)
    Working(Worker)

    pub def started(self: &mut Node, context: &mut Context[Job, Fault]) -> Result[(), Fault]:
        match self:
            case Node.Leading(boss):
                boss.me = Some(context.address())
                good = context.spawn(1, 65536).unwrap()
                bad = context.spawn(2, 65536).unwrap()
                context.send(good, Job.Square(7)).unwrap()
                context.send(bad, Job.Square(-1)).unwrap()
            case Node.Working(_):
                pass
        Ok(())

    pub def receive(self: &mut Node, mail: Mail[Job, Fault], context: &mut Context[Job, Fault]) -> Result[(), Fault]:
        match self:
            case Node.Leading(boss):
                match mail:
                    case Mail.Message(Job.Answer(value)):
                        print("answer", value)?
                    case Mail.Down(_, Exit.Stopped):
                        print("worker stopped")?
                        boss.ended += 1
                    case Mail.Down(_, Exit.Trapped):
                        print("worker trapped")?
                        boss.ended += 1
                    case _:
                        pass
                if boss.ended == 2:
                    context.stop()
            case Node.Working(worker):
                match mail:
                    case Mail.Message(Job.Square(value)):
                        if value < 0:
                            # A bug: unwrapping Nothing traps.
                            missing: Option[i64] = Nothing
                            missing.unwrap()
                        context.send(worker.boss, Job.Answer(value * value)).unwrap()
                        context.stop()
                    case _:
                        pass
        Ok(())

    # Runs with the new child's heap current.
    pub def spawn(self: &mut Node, tag: i64, child: Address, context: &mut Context[Job, Fault]) -> Result[Option[Node], Fault]:
        match self:
            case Node.Leading(boss):
                Ok(Some(Node.Working(Worker(boss.me.unwrap()))))
            case Node.Working(_):
                Ok(Nothing)

def main() -> Result[(), AllocError | IoError | WaitError]:
    mut actors = stage[Node, Job, Fault](8, 4, 4)?
    root = Node.Leading(Boss(Nothing, 0))
    actors.start(root, 65536).unwrap()
    actors.run(&deadline_after(&monotonic_now(), &seconds(5)).unwrap(), &CancellationToken()?)?
    stats = actors.stats()
    print(stats.started, stats.stopped, stats.trapped, heaps_live(), heap_bytes_used())?
    Ok(())
```
```output
answer 49
worker stopped
worker trapped
3 2 1 0 0
```

The trap also prints its usual message to standard error, followed by
`note: contained in an actor; its heap is discarded`.

The repository's `examples/actor_echo.plenty` is a TCP echo server with one actor per connection; a connection whose first
byte is `X` traps, and only that connection closes.

## Actors and stages

A `Stage[N, M, E]` runs actors of one node type `N`, message type `M` and error
type `E`. `N` is a public class or enum with three public methods:

| Method | Called |
|---|---|
| `started(self: &mut N, context: &mut Context[M, E]) -> Result[(), E]` | Once, before any mail |
| `receive(self: &mut N, mail: Mail[M, E], context: &mut Context[M, E]) -> Result[(), E]` | For each delivered mail |
| `spawn(self: &mut N, tag: i64, child: Address, context: &mut Context[M, E]) -> Result[Option[N], E]` | On the parent, after a handler that called `Context.spawn` returns; builds the child with the child's heap current. `Nothing` stops the child at once |

`M` and `E` must be copyable with `copy`; delivery copies them into another
heap. Use an enum node type to run several kinds of actor on one stage, as in
the example.

| Export | Contract |
|---|---|
| `stage[N, M, E](capacity, mailbox, events)` | `Result[Stage[N, M, E], AllocError \| WaitError]`: at most `capacity` live actors, `mailbox` messages per actor, `events` ready handles per wait. Allocates every slot table now |
| `Stage.start(node, limit)` | `Result[Address, StartError]`: a root actor with a heap of `limit` bytes. Allocates its mailbox now |
| `Stage.send(to, message)` | `Result[(), Undelivered[M]]` |
| `Stage.stop_actor(to)` | `bool`: queues `Mail.Stop`; `False` when the actor has stopped or its mailbox is full |
| `Stage.run(&deadline, &cancel)` | `Result[(), WaitError]`: delivers mail and readiness until no actor is live; fails when the deadline passes or `cancel` is cancelled first |
| `Stage.live()` | Live actors |
| `Stage.stats()` | `Stats` |
| `Stage.heap_used(actor)` | `Option[i64]`: bytes in the actor's heap, `Nothing` once it has stopped |
| `Context.address()` | The running actor's `Address` |
| `Context.send(to, message)` | `Result[(), Undelivered[M]]` |
| `Context.stop()` | Stops the running actor after this handler returns |
| `Context.stop_actor(to)` | As `Stage.stop_actor` |
| `Context.spawn(tag, limit)` | `Result[Address, StartError]`: reserves a child slot, heap and mailbox now; the parent's `spawn` hook builds the child after the handler returns |
| `Context.heap_used()` | Bytes in the running actor's heap |
| `Context.watch_stream(&stream, interest)`, `unwatch_stream(&stream)` | `Result[(), NetError]`: delivers the stream's readiness as `Mail.Ready` |
| `Context.watch_listener(&listener)`, `unwatch_listener(&listener)` | `Result[(), NetError]`: `Mail.Ready` while a connection waits |
| `supported()` | Whether this target has actor heaps |
| `heaps_live()`, `heap_bytes_used()`, `heap_bytes_committed()` | Process totals over all actor heaps |
| `max_heap_limit` | 64 MiB, the largest heap limit |
| `max_spawns` | 16 spawn requests per handler |

| Type | Values |
|---|---|
| `Address` | A slot and a generation. A stopped actor's address is never reused; `key()` returns it as `u64` |
| `Mail[M, E]` | `Message(M)`, `Ready(Readiness)`, `Down(Address, Exit[E])`, `Stop` |
| `Exit[E]` | `Stopped`, `Failed(E)`, `Trapped` |
| `Undelivered[M]` | `Full(M)`: the mailbox is full or the receiver's heap cannot hold the copy. `Gone(M)`: the receiver has stopped |
| `StartError` | `Unsupported`, `Capacity` (no free slot, or `max_spawns` reached), `Heaps` (1024 heaps live or the reservation failed), `Limit` (not in 1..`max_heap_limit`), `Mailbox` (its allocation failed) |
| `Stats` | `started`, `stopped`, `failed`, `trapped`, `delivered`, `full` (refused sends, and readiness deferred by a full mailbox), `gone` (sends and readiness for stopped actors), `lost` (`Down` for a parent that had stopped) |

### Delivery

Each round delivers one mail to every actor that was runnable when the round
began, then waits on the stage's `std.ready` selector without blocking if any
actor is still runnable. Readiness is level-triggered: an actor has at most one
`Mail.Ready` queued, and readiness that finds a full mailbox is reported again
on a later wait.

`Stop` stops the actor after its handler returns. An `Err(error)` from any
handler stops the actor and gives its parent `Exit.Failed(error)`, copied into
the parent's heap; if that copy does not fit, the parent sees `Exit.Trapped`.
Destructors run for a stopped or failed actor's node.

### Supervision

Every actor started by `Context.spawn` has a parent; the parent receives
`Mail.Down(child, exit)` after the child's last message, so a reply always
arrives before the `Down` that follows it. `Down` never needs mailbox room and
is never refused: the child's slot holds the exit until the parent receives
it, so a stopped child occupies one unit of `capacity` until then. When the
parent has already stopped, the exit is dropped and counted in `Stats.lost`. A
stopping actor's undelivered mail is dropped, and so are its children's
pending exits.

Restart policy is the parent's code: on `Down`, call `Context.spawn` again.

## Memory accounting

Each heap reserves a 64 MiB address range and commits pages as its size
classes grow. A heap's limit counts the bytes of its live blocks, each rounded
up to its size class; an allocation over the limit fails like any other
allocation, as `AllocError` where the program handles it. A handler that
returns that error fails its actor:

```plenty
from std.actor import Address, Context, Mail, heap_bytes_used, heaps_live, stage
from std.ready import WaitError
from std.sync import CancellationToken
from std.time import deadline_after, monotonic_now, seconds

type Fault = AllocError | IoError

pub class Greedy:
    pub size: i64

    pub def started(self: &mut Greedy, context: &mut Context[i64, Fault]) -> Result[(), Fault]:
        # 8 bytes for each of a million values do not fit in 64 KiB.
        values: list[i64] = [0 for _ in range(self.size)]?
        print("unreachable", len(values))?
        Ok(())

    pub def receive(self: &mut Greedy, mail: Mail[i64, Fault], context: &mut Context[i64, Fault]) -> Result[(), Fault]:
        Ok(())

    pub def spawn(self: &mut Greedy, tag: i64, child: Address, context: &mut Context[i64, Fault]) -> Result[Option[Greedy], Fault]:
        Ok(Nothing)

def main() -> Result[(), AllocError | IoError | WaitError]:
    mut actors = stage[Greedy, i64, Fault](1, 1, 1)?
    actors.start(Greedy(1000000), 65536).unwrap()
    actors.run(&deadline_after(&monotonic_now(), &seconds(5)).unwrap(), &CancellationToken()?)?
    print(actors.stats().failed, heaps_live(), heap_bytes_used())?
    Ok(())
```
```output
1 0 0
```

An allocation failure the handler does not handle, such as one inside a
runtime operation that cannot report it, is contained like a trap.

`heap_bytes_used()` is the sum of every heap's used bytes and
`heap_bytes_committed()` the pages made writable; discarding a heap returns
both. At most 1024 heaps are live per process.

The stage's own tables, every mailbox, and `Context` live outside all actor
heaps and hold no pointer into one, so discarding a heap cannot leave a
dangling pointer in them. The root node passed to `Stage.start` was built
outside its heap: whatever it already owns stays outside and leaks if the
actor traps. Build large state in `started` instead.

## Traps

A trap in a handler, including an allocation failure that cannot be returned:

- discards the actor's heap, returning its memory;
- closes up to 4096 files and sockets the actor opened, which also removes
  them from the stage's selector;
- gives the parent `Exit.Trapped`;
- does not run destructors for the trapped actor's node or the handler's
  locals, since they lived in the discarded heap.

The trap kind and message are not reported to the parent; they appear on
standard error. These are not contained and still end the process:

- stack overflow;
- internal runtime panics;
- an allocation failure while no handler is running.

A lock or other process-wide state held at the moment of a trap stays as it
was. Allocations made by C libraries through FFI, such as SQLite's, are outside
actor heaps and are neither limited nor freed by a trap.

## Limits

- One thread. Heaps belong to the thread that created the first one, and
  heap operations from another thread fail. Do not start threads or pools in
  a handler.
- The `spawn` hook runs the parent's code with the child's heap current. It can
  move values between the two heaps, and the compiler does not reject this.
  Create values the child owns, such as an accepted socket, inside the hook,
  and keep nothing the hook allocated in the parent. A test pins this escape
  until the checker confines the hook. In the hook, `Context.spawn` returns
  `StartError.Capacity` and `Context.stop` has no effect.
- Copying a message copies owned values. Where `copy` would share storage
  instead, a `str` is duplicated and any other shared value stops the program
  with a runtime error.
- One message type per stage. There is no request-reply helper, timer mail, or
  restart strategy; supervision is ordinary parent code.
- Files beyond 4096 per process are not tracked and stay open after a trap.
- Handlers are not preempted: a handler that loops forever stalls the stage.

## Acknowledgements

- Parents receiving their children's exits follow Erlang/OTP
  [supervisors](https://www.erlang.org/doc/apps/stdlib/supervisor.html); no
  Erlang code is used.
