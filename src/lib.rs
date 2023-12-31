#![feature(local_key_cell_methods)]

mod error;
mod runtime;
mod platform;
mod join_handle;

pub mod fs;
pub mod time;
pub mod net;
pub mod util;
pub use join_handle::JoinHandle;

use std::ptr;
use std::pin::pin;
use std::future::Future;
use std::cell::{Cell, RefCell};
use std::task::{Poll, Context, Waker, RawWaker, RawWakerVTable};

pub use error::UringError;
use runtime::{Runtime, WokenTask};


thread_local! {
    // The lazy initializer panics, therefore enforcing that the runtime is initialized only using init()
    // This works because init() uses .set() which does not run the lazy initializer
    pub(crate) static RUNTIME: RefCell<Runtime> = panic!("init() has not been called on this thread!");

    pub(crate) static RUNNING: Cell<bool> = const { Cell::new(false) };
}

static WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(|_| panic!(), |_| (), |_| (), |_| ());

/// Initializes the thread-local runtime
/// 
/// This must be called at least once before calling [`run()`] on a thread
pub fn init() -> Result<(), UringError> {
    RUNTIME.set(Runtime::new()?);
    Ok(())
}

/// Runs a future on the current thread, blocking it whenever waiting for IO
/// The passed future is the root task, which will be polled until it finishes
/// It can spawn more tasks using [`spawn()`] to spawn child tasks. It is dropped when the root task finishes.
/// All child tasks must finish before the root task finishes, otherwise they will be dropped.
/// If necessary for the root task to wait for a child task, it can await on the child's 
/// [`JoinHandle`] returned by [`spawn()`].
pub fn run<F: Future>(root_task: F) -> F::Output {
    RUNNING.set(true);

    let waker = unsafe { Waker::from_raw(RawWaker::new(ptr::null(), &WAKER_VTABLE)) };
    let mut cx = Context::from_waker(&waker);
    
    let mut root_task = pin!(root_task);

    loop {
        // Poll all woken up tasks
        loop {
            let task = RUNTIME.with_borrow_mut(|rt| rt.get_woken_task());

            match task {
                // Root task woken up
                Some(WokenTask::Root) => {
                    let poll = root_task.as_mut().poll(&mut cx);

                    // Root task finished, reset runtime and return
                    if let Poll::Ready(res) = poll {
                        RUNTIME.with_borrow_mut(|rt| rt.reset());
                        RUNNING.set(false);
                        return res;
                    }
                },
    
                // Child task woken up
                Some(WokenTask::Child(mut task)) => {
                    let poll = task.as_mut().poll(&mut cx);

                    match poll {
                        // Child task pending, return it into task list
                        Poll::Pending => RUNTIME.with_borrow_mut(|rt| rt.return_task(task)),
                        
                        // Child task finished with result
                        Poll::Ready(res) => RUNTIME.with_borrow_mut(|rt| rt.task_finished(res))
                    }
                },

                // No more woken tasks left
                None => break
            }
        }

        // Wait for IO events to wake up more tasks
        RUNTIME.with_borrow_mut(|rt| rt.wait_for_io());
    }
}

pub fn spawn<F: Future + 'static>(task: F) -> JoinHandle<F::Output> {
    if !RUNNING.get() {
        panic!("spawn() called outside of a run() call!")
    }

    RUNTIME.with_borrow_mut(|rt| rt.spawn(task))
}