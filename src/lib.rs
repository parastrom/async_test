mod error;
mod platform;
mod executor;

pub mod time;
pub mod fs;

use std::ptr;
use std::pin::pin;
use std::future::Future;
use std::task::{Poll, Context, Waker, RawWaker, RawWakerVTable};

pub use error::UringError;
pub use executor::Executor;

static WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(|_| panic!(), |_| (), |_| (), |_| ());

pub fn run_raw<F, B, O>(builder: B) -> Result<O, UringError>
where
    F: Future<Output = O>,
    B: Fn(*const Executor) -> F
{
    let waker = unsafe { Waker::from_raw(RawWaker::new(ptr::null(), &WAKER_VTABLE)) };
    let mut cx = Context::from_waker(&waker);

    let exec = Executor::new()?;
    let mut fut = pin!(builder(&exec));

    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Pending => exec.wait_for_events()?,
            Poll::Ready(val) => return Ok(val)
        }
    }
}

#[macro_export]
macro_rules! run {
    (async $(move $(@$move:tt)?)? |$arg:tt $(: $ArgTy:ty)? $(,)?| $(-> $Ret:ty)? $body:block) => {
        $crate::run_raw(|arg: *const $crate::Executor| async move {
            // Hack solution to the problem of async blocks not being able to capture references
            // On stable rust, the compiler cannot guarantee that the async block returned by the closure 
            // does not outlive the Executor reference it's given, since it cannot see the closure. Therefore,
            // it will not allow the async block to capture the reference. However, we know that the async
            // block will not outlive the reference, since it is returned by the closure and therefore
            // cannot outlive it. We also know that the async block will not outlive the Executor reference
            // because it is tied to the lifetime of the reference. The compiler cannot see this, however,
            // and so complains/errors out.
            // We solve this by using an a raw pointer and unsafely converting it to a reference. This
            // allows us to capture the reference without the compiler complaining. This ties the lifetime
            // of the async block to the lifetime of the reference, which is what we want.            
            let local = [];

            let arg: &$crate::Executor = if true {
                unsafe { &*arg }
            }
            else {
                &local[0]
            };

            let $arg $(: $ArgTy)? = arg;

            $body
        })
    };
}