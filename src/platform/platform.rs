use io_uring::{IoUring, opcode, squeue};
use crate::runtime::TaskId;
use crate::error::UringError;
use nohash::IntMap;
use super::IoKey;
use io_uring::Probe;

fn new_io_uring() -> Result<IoUring, UringError> {
    let ring = IoUring::new(128).map_err(|err| UringError::FailedInit(err))?;

    if !ring.params().is_feature_nodrop() {
        return Err(UringError::UnsupportedFeature("no_drop"));
    }

    // Probe supported opcodes
    let mut probe = Probe::new();

    ring.submitter()
        .register_probe(&mut probe)
        .map_err(|err| UringError::ProbeFailed(err))?;

    // Check required opcodes
    let req_opcodes = [
        ("AsyncCancel", opcode::AsyncCancel::CODE),
        ("Timeout", opcode::Timeout::CODE),
        ("Socket", opcode::Socket::CODE),
        ("Connect", opcode::Connect::CODE),
        ("RecvMsg", opcode::RecvMsg::CODE),
        ("SendMsg", opcode::SendMsg::CODE),
        ("Shutdown", opcode::Shutdown::CODE),
        ("OpenAt", opcode::OpenAt::CODE),
        ("Read", opcode::Read::CODE),
        ("Write", opcode::Write::CODE),
        ("Close", opcode::Close::CODE)
    ];

    for (name, code) in req_opcodes {
        if !probe.is_supported(code) {
            return Err(UringError::UnsupportedOpcode(name));
        }
    }

    Ok(ring)
}


pub struct Platform {
    ring: IoUring,
    io_key_counter: IoKey,

    pub (crate) submissions: IntMap<IoKey, TaskId>,
    pub (crate) completions: IntMap<IoKey, i32>,
}

impl Platform {
    pub fn new() -> Result<Self, UringError> {
        Ok(Self {
            ring: new_io_uring()?,
            io_key_counter: 1, // 0 is reserved for the close operations
            submissions: IntMap::default(),
            completions: IntMap::default()
        })
    }

    pub fn wait_for_io(&mut self, wakeups: &mut Vec<TaskId>) {
        self.ring
            .submit_and_wait(1)
            .expect("Failed to submit io_uring");


        for cqe in self.ring.completion() {
            let key = IoKey::from(cqe.user_data() as u32);

            if let Some(task_id) = self.submissions.remove(&key) {
                self.completions.insert(key, cqe.result());
                wakeups.push(task_id);
            }
        }
    }

    pub fn reset(&mut self) {
        // To get rid of pending IO we drop the current io_uring and
        // reset to our original state, we don't handle UringErrors
        // because since by this point `new()` has been called
        // successfully it is unlikely to return an error now
        *self = Self::new().unwrap();
    }

    pub (crate) fn new_io_key(&mut self) -> IoKey {
        let key = self.io_key_counter;
        self.io_key_counter = key.wrapping_add(1);

        if self.io_key_counter == 0 {
            self.io_key_counter = 1;
        }

        key
    }

    pub (crate) fn submit_sqe(&mut self, sqe: squeue::Entry) {
        loop {
            // Try and push the sqe
            let res = unsafe {
                self.ring
                    .submission()
                    .push(&sqe)
            };

            match res {
                // Push successful, return
                Ok(()) => return,
                
                // No space left in submission queue
                // Submit it to make space and try again
                Err(_) => { if self.io_key_counter == 0 {
                    self.io_key_counter = 1;
                }
                    self.ring
                        .submit()
                        .expect("Failed to submit io_uring");
                }
            }
        }
    }

}