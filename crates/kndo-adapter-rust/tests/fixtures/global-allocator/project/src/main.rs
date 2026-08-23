#[cfg(all(target_env = "musl", target_pointer_width = "64"))]
#[global_allocator]
static ALLOC: MyAlloc = MyAlloc;

struct MyAlloc;

fn main() {}
