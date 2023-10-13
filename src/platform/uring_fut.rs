use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use io_uring::{squeue, opcode};
use crate::RUNTIME;
use super::IoKey;

#[derive(Clone, Copy)]
enum FutState {
    NotSubmitted,
    Submitted(IoKey),
    Done
}

pub (crate) struct UringFut {
    sqe: squeue::Entry,
    state: FutState
}

impl UringFut {
    pub fn new(sqe: squeue::Entry) -> Self {
        Self { sqe, state: FutState::NotSubmitted }
    }
}


impl Future for UringFut {
    type Output = i32;

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.state {
            // sqe not submitted yet
            FutState::NotSubmitted => RUNTIME.with_borrow_mut(|rt| {
                let key = rt.plat.new_io_key();
                let sqe = self.sqe.clone().user_data(key as u64);

                rt.plat.submit_sqe(sqe);
                rt.plat.submissions.insert(key, rt.current_task);
                self.state = FutState::Submitted(key);

                Poll::Pending
            }),

            // sqe submitted, query it
            FutState::Submitted(key) => RUNTIME.with_borrow_mut(|rt| {
                match rt.plat.completions.remove(&key) {
                    Some(res) => {
                        self.state = FutState::Done;
                        Poll::Ready(res)
                    },
                    None => Poll::Pending
                }
            }),

            FutState::Done => panic!("IoRingFut polled even after completing")
        }
    }
}

impl Drop for UringFut {
    fn drop(&mut self) {
        if let FutState::Submitted(key) = &self.state {
            RUNTIME.with_borrow_mut(|rt| {
                if rt.plat.submissions.remove(key).is_some() {
                    let sqe = opcode::AsyncCancel::new(*key as u64).build();
                    rt.plat.submit_sqe(sqe);   
                }
            });
        }
    }
}