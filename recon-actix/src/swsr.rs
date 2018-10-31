/**
 * Single-Writer Single-Reader Shared Register
 */

type Value = u32;
type Clock = u32;

struct RemoteProcess;

#[derive(Message)]
struct Write {
    v: Value,
    t: Clock,
}

struct Writer {
    t: Clock,
    n: usize,
    f: usize,
    processes: Vec<Address<RemoteProcess>>,
}

impl Actor for Writer {
    type Context = Context<Self>;
}

impl Writer {

}