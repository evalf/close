[![Documentation](https://docs.rs/close/badge.svg)](https://docs.rs/close/)
[![Crates.io](https://img.shields.io/crates/v/close.svg)](https://crates.io/crates/close)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE-MIT)

Rust trait + smart pointer for manual object destruction

This crate introduces the `Close` trait for manual object destruction. While
similar in purpose to `Drop`, the key difference is that the `close` method
takes ownership over `self` and allows for error propagation. For use in
conjuction with `Drop`, this crate further introduces the `Closing<T: Close>`
smart pointer, which is a zero cost abstraction that closes the contained
object upon drop if it was not closed manually.

## Motivation

Having ownership over `self` is useful in situations where the destruction
sequence requires dropping or moving members. With `drop` this requires
solutions such as sticking the member in an `Option`, as in the following
example, which joins a thread (moving its handle) before continuing the
teardown process. The downside of this construction is added runtime cost and
reduced ergonomics in accessing the data behind the option.

```rust
struct DeepThought(Option<std::thread::JoinHandle<u32>>);

impl DeepThought {
    fn new() -> Self {
        Self(Some(std::thread::spawn(|| 42)))
    }
    fn thread(&self) -> &std::thread::Thread {
        self.0.as_ref().unwrap().thread() // <-- not great
    }
}

impl Drop for DeepThought {
    fn drop(&mut self) {
        match self.0.take().unwrap().join() {
            Err(e) => std::panic::resume_unwind(e),
            Ok(_answer) => /*... teardown ...*/ ()
        }
    }
}
```

Using `close` instead of `drop` we can avoid the option dance and write things
as one naturally would:

```rust
use close::{Close, Closing};

struct DeepThought(std::thread::JoinHandle<u32>);

impl DeepThought {
    fn new() -> Closing<Self> {
        Self(std::thread::spawn(|| 42)).into()
    }
    fn thread(&self) -> &std::thread::Thread {
        self.0.thread() // <-- better!
    }
}

impl Close for DeepThought {
    type Error = String;
    fn close(self) -> Result<(), Self::Error> {
        match self.0.join() {
            Err(e) => Err(format!("thread panicked: {:?}", e)),
            Ok(_answer) => /*... teardown ...*/ Ok(()),
        }
    }
}
```

Note that besides avoiding `Option`, the constructor now returns the `Closing`
smart pointer. As a result, the second implementation can be used in precisely
the same way as the former, using automatic dereferencing to access members and
methods and joining the thread when the object goes out of scope. The
difference is that the latter allows for a more ergonomic implementation, does
not incur any runtime cost, and allows for manual closing in case error
handling is desired.
