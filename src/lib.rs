//! Trait + smart pointer for manual object destruction
//!
//! This crate introduces the [`Close`] trait for manual object destruction.
//! While similar in purpose to [`Drop`], the key difference is that the `close`
//! method takes ownership over `self` and allows for error propagation. For use
//! in conjuction with [`Drop`], this crate further introduces the [`Closing<T:
//! Close>`] smart pointer, which is a zero cost abstraction that closes the
//! contained object upon drop if it was not closed manually.
//!
//! # Motivation
//!
//! Having ownership over `self` is useful in situations where the destruction
//! sequence requires dropping or moving members. With `drop` this requires
//! solutions such as sticking the member in an [`Option`], as in the following
//! example, which joins a thread (moving its handle) before continuing the
//! teardown process. The downside of this construction is added runtime cost
//! and reduced ergonomics in accessing the data behind the option.
//!
//! ```
//! struct DeepThought(Option<std::thread::JoinHandle<u32>>);
//!
//! impl DeepThought {
//!     fn new() -> Self {
//!         Self(Some(std::thread::spawn(|| 42)))
//!     }
//!     fn thread(&self) -> &std::thread::Thread {
//!         self.0.as_ref().unwrap().thread() // <-- not great
//!     }
//! }
//!
//! impl Drop for DeepThought {
//!     fn drop(&mut self) {
//!         match self.0.take().unwrap().join() {
//!             Err(e) => std::panic::resume_unwind(e),
//!             Ok(_answer) => /*... teardown ...*/ ()
//!         }
//!     }
//! }
//! ```
//!
//! Using `close` instead of `drop` we can avoid the option dance and write
//! things as one naturally would:
//!
//! ```
//! use close::{Close, Closing};
//!
//! struct DeepThought(std::thread::JoinHandle<u32>);
//!
//! impl DeepThought {
//!     fn new() -> Closing<Self> {
//!         Self(std::thread::spawn(|| 42)).into()
//!     }
//!     fn thread(&self) -> &std::thread::Thread {
//!         self.0.thread() // <-- better!
//!     }
//! }
//!
//! impl Close for DeepThought {
//!     type Error = String;
//!     fn close(self) -> Result<(), Self::Error> {
//!         match self.0.join() {
//!             Err(e) => Err(format!("thread panicked: {:?}", e)),
//!             Ok(_answer) => /*... teardown ...*/ Ok(()),
//!         }
//!     }
//! }
//! ```
//!
//! Note that besides avoiding [`Option`], the constructor now returns the
//! [`Closing`] smart pointer. As a result, the second implementation can be
//! used in precisely the same way as the former, using automatic dereferencing
//! to access members and methods and joining the thread when the object goes
//! out of scope. The difference is that the latter allows for a more ergonomic
//! implementation, does not incur any runtime cost, and allows for manual
//! closing in case error handling is desired.

pub trait Close {
    /// Defines the `close` method for manual object destruction.
    ///
    /// # Example
    ///
    /// ```
    /// struct MyIOStruct;
    ///
    /// impl close::Close for MyIOStruct {
    ///     type Error = std::io::Error;
    ///     fn close(self) -> std::io::Result<()> {
    ///         // ... fallible i/o code ...
    ///         Ok(())
    ///     }
    /// }

    type Error: std::fmt::Debug;
    fn close(self) -> Result<(), Self::Error>;
}

/// A zero-cost smart pointer that closes on drop.
///
/// # Example
///
/// ```
/// use close::{Close, Closing};
///
/// struct MyIOStruct;
///
/// impl MyIOStruct {
///     fn new() -> Closing<Self> {
///         Self.into()
///     }
///     fn say_hello(&self) {
///         println!("hello");
///     }
/// }
///
/// impl Close for MyIOStruct {
///     type Error = std::io::Error;
///     fn close(self) -> std::io::Result<()> {
///         // ... fallible i/o code ...
///         Ok(())
///     }
/// }
///
/// fn main() -> std::io::Result<()> {
///     let s = MyIOStruct::new();
///     s.say_hello(); // automatic dereferencing
///     s.close()?; // manual closing
///     let t = MyIOStruct::new();
///     Ok(())
/// } // closing t on drop
#[derive(Debug)]
pub struct Closing<T: Close>(std::mem::MaybeUninit<T>);

impl<T: Close> Closing<T> {
    unsafe fn uninit(&mut self) -> T {
        // Retrieve value from MaybeUninit and replace it by uninit. This
        // private method is the only routine that uninitializes self. Since it
        // is used only from drop or prior to mem::forget, we can safely assume
        // init for the duration of the object's lifetime.
        std::mem::replace(&mut self.0, std::mem::MaybeUninit::uninit()).assume_init()
    }
    /// Consumes the `Closing`, returning the wrapped value.
    pub fn into_inner(mut self) -> T {
        // We cannot simply return self.0.assume_init because self implements
        // the Drop trait. Instead, we swap out the contents and then forget
        // about self to avoid a segfault in drop.
        let inner = unsafe { self.uninit() }; // safe because we call mem:forget next
        std::mem::forget(self);
        inner
    }
}

impl<T: Close> std::convert::From<T> for Closing<T> {
    fn from(value: T) -> Closing<T> {
        Closing(std::mem::MaybeUninit::new(value))
    }
}

impl<T: Close> std::ops::Deref for Closing<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { self.0.assume_init_ref() }
    }
}

impl<T: Close> std::ops::DerefMut for Closing<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.0.assume_init_mut() }
    }
}

impl<T: Close> Close for Closing<T> {
    type Error = T::Error;
    fn close(self) -> Result<(), Self::Error> {
        self.into_inner().close()
    }
}

impl<T: Close> Drop for Closing<T> {
    fn drop(&mut self) {
        let inner = unsafe { self.uninit() }; // safe because we are in drop
        inner.close().expect("failed to close on drop");
    }
}

// Default implementations

impl<T0: Close> Close for (T0,) {
    type Error = T0::Error;
    fn close(self) -> Result<(), Self::Error> {
        self.0.close()
    }
}

impl<T0: Close, T1: Close> Close for (T0, T1) {
    type Error = (Option<T0::Error>, Option<T1::Error>);
    fn close(self) -> Result<(), Self::Error> {
        let result = (self.0.close().err(), self.1.close().err());
        if result.0.is_none() && result.1.is_none() {
            Ok(())
        }
        else {
            Err(result)
        }
    }
}

impl<T0: Close, T1: Close, T2: Close> Close for (T0, T1, T2) {
    type Error = (Option<T0::Error>, Option<T1::Error>, Option<T2::Error>);
    fn close(self) -> Result<(), Self::Error> {
        let result = (self.0.close().err(), self.1.close().err(), self.2.close().err());
        if result.0.is_none() && result.1.is_none() && result.2.is_none() {
            Ok(())
        }
        else {
            Err(result)
        }
    }
}

impl<T0: Close, T1: Close, T2: Close, T3: Close> Close for (T0, T1, T2, T3) {
    type Error = (Option<T0::Error>, Option<T1::Error>, Option<T2::Error>, Option<T3::Error>);
    fn close(self) -> Result<(), Self::Error> {
        let result = (self.0.close().err(), self.1.close().err(), self.2.close().err(), self.3.close().err());
        if result.0.is_none() && result.1.is_none() && result.2.is_none() && result.3.is_none() {
            Ok(())
        }
        else {
            Err(result)
        }
    }
}

impl<T: Close> Close for Vec<T> {
    type Error = Vec<Option<T::Error>>;
    fn close(self) -> Result<(), Self::Error> {
        let result: Self::Error = self.into_iter().map(|item| item.close().err()).collect();
        if result.iter().all(|item| item.is_none()) {
            Ok(())
        }
        else {
            Err(result)
        }
    }
}

impl<T: Close> Close for Box<T> {
    type Error = T::Error;
    fn close(self) -> Result<(), Self::Error> {
        (*self).close()
    }
}

impl<T: Close> Close for Option<T> {
    type Error = T::Error;
    fn close(self) -> Result<(), Self::Error> {
        if let Some(v) = self {
            v.close()
        }
        else {
            Ok(())
        }
    }
}

impl Close for std::fs::File {
    type Error = std::io::Error;
    fn close(self) -> std::io::Result<()> {
        // From the docs: Files are automatically closed when they go out of
        // scope. Errors detected on closing are ignored by the implementation
        // of Drop. Use the method sync_all if these errors must be manually
        // handled.
        self.sync_all()
    }
}
