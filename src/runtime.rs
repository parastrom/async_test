use std::io;
use std::mem;
use std::any::Any;
use std::pin::Pin;
use std::future::Future;
use std::time::Duration;
use std::os::fd::AsRawFd;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};

use nohash::IntMap;

use crate::{
    JoinHandle,
    platform::Platform,
    error::UringError,
};

pub type TaskId = u32;

#[cfg(unix)]
pub struct SocketHandle(pub std::os::fd::RawFd);

impl From<&UdpSocket> for SocketHandle {
    fn from(value: &UdpSocket) -> Self {
        #[cfg(unix)]
        Self(value.as_raw_fd())
    }
}

type Task = Pin<Box<dyn Future<Output = Box<dyn Any>>>>;

pub enum WokenTask {
    Root,
    Child(Task)
}

struct JoinHandleInfo {
    result: Option<Box<dyn Any>>,
    waiting_task: Option<TaskId>
}

pub struct Runtime {
    task_id_counter: TaskId,

    pub current_task: TaskId,
    tasks: IntMap<TaskId, Task>,
    join_handles: IntMap<TaskId, JoinHandleInfo>,
    task_wakeups: Vec<TaskId>,

    pub plat: Platform,
}

impl Runtime {
    pub fn new() -> Result<Self, UringError> {
        let plat = Platform::new()?;

        Ok(Self {
            task_id_counter: 1,
            current_task: 0,
            tasks: IntMap::default(),
            join_handles: IntMap::default(),
            task_wakeups: vec![0], // We always start with the root task already woken up
            plat
        })
    }

    fn new_task_id(&mut self) -> TaskId {
        let id = self.task_id_counter;
        self.task_id_counter += id.wrapping_add(1);

        // TaskId 0 is reserved for the root task
        if self.task_id_counter == 0 {
            self.task_id_counter = 1;
        }

        id
    }

    pub fn reset(&mut self) -> IntMap<TaskId, Task> {
        self.task_id_counter = 1;
        self.current_task = 0;
        self.join_handles = IntMap::default();
        self.task_wakeups = vec![0];
        self.plat.reset();

        // We replace and transfer task ownership to `run()`, avoiding double borrows of the runtime. 
        // This allows tasks to be dropped in `run()`, ensuring exclusive runtime access for each task,
        // even during IO cancellation.
        mem::replace(&mut self.tasks, IntMap::default())
    }

    pub fn wait_for_io(&mut self) {
        self.plat.wait_for_io(&mut self.task_wakeups)
    }
}


impl Runtime {

    /// Spawns a task - The task is placed in the task list and also placed 
    /// in the wakeup list to give it an initial poll.
    /// 
    /// Returns a join handle to the task
    pub fn spawn<F: Future + 'static>(&mut self, task: F) -> JoinHandle<F::Output> {
        let id = self.new_task_id();

        // Wrap the task to erase its output type
        let wrapped_task = async {
            let res: Box<dyn Any> = Box::new(task.await);
            res
        };

        // Place the task in the task list, create it's join handle, and also place in
        // the wakeup list to give it an initial poll
        self.tasks.insert(id, Box::pin(wrapped_task));
        self.join_handles.insert(id, JoinHandleInfo { result: None, waiting_task: None });
        self.task_wakeups.push(id);

        JoinHandle::new(id)
    }

    /// Gets a woken up task from the wakeup list
    /// Returns `None` if there are no more woken up tasks.
    /// 
    /// If the task is the root task, returns `WokenTask::RootTask`, otherwise returns
    /// `WokenTask::ChildTask(task)`.
    pub fn get_woken_task(&mut self) -> Option<WokenTask> {

        loop {
            let id = self.task_wakeups.pop()?;
            self.current_task = id;

            self.current_task = id;
            if id == 0 {
                return Some(WokenTask::Root);
            }
            else {
                // A woken up task id  may not be in the task list if it was woken up
                // earlier before it's completion, and then completed before it was polled.
                // In this case, we just ignore it and continue.
                match self.tasks.remove(&id) {
                    Some(task) => return Some(WokenTask::Child(task)),
                    None => continue
                }
            }
        }
    }


    /// Marks the current task as finished and stores its result into its join handle.
    /// 
    /// Also places the ID of the task waiting on the current task's join handle into
    /// the wakeup list
    pub fn task_finished(&mut self, res: Box<dyn Any>) {
        match self.join_handles.get_mut(&self.current_task) {
            // Write result into join handle and wake up it's waiting task, if any
            Some(handle) => {
                handle.result = Some(res);

                if let Some(id) = handle.waiting_task {
                    self.task_wakeups.push(id);
                }
            },

            // Join handle dropped, discard result
            None => ()
        }
    }

    /// Returns a task to the task list
    pub fn return_task(&mut self, task: Task) {
        self.tasks.insert(self.current_task, task);
    }
}


impl Runtime {

    /// Tries to retrieve a join handle's result and remove it from the list if it's done
    /// Returns `None if the join handle is not done yet
    pub fn pop_join_handle_result(&mut self, id: TaskId) -> Option<Box<dyn Any>> {
        let info = self.join_handles.remove(&id).expect("Join handle info not found");

        if let Some(res) = info.result {
            Some(res)
        }
        else {
            self.join_handles.insert(id, info);
            None
        }
    }


    /// Registers a task to be woken up when the task with the given ID completes.
    /// 
    /// This is called when a task is spawned and a join handle is created for it.
    pub fn register_join_handle_wakeup(&mut self, id: TaskId) {
        let info = self.join_handles.get_mut(&id).expect("Join handle info not found");
        info.waiting_task = Some(self.current_task);
    }

    /// Drops a join handle - this is called when a join handle is dropped, and is used to
    /// remove the join handle info from the list so that the task's result is discarded
    pub fn drop_join_handle(&mut self, id: TaskId) {
        self.join_handles.remove(&id);
    }
}


impl Runtime {

    pub fn sleep_fut(&mut self, dur: Duration) -> impl Future<Output = ()> {
        self.plat.sleep_fut(dur)
    }


    pub fn recv_from_fut<'a>(&self, sock: SocketHandle, buf: &'a mut [u8], peek: bool) -> impl Future<Output = io::Result<(usize, SocketAddr)>> + 'a {
        self.plat.recv_from_fut(sock, buf, peek)
    }

    pub fn send_fut<'a>(&self, sock: SocketHandle, buf: &'a [u8]) -> impl Future<Output = io::Result<usize>> + 'a {
        self.plat.send_to_fut(sock, buf, None)
    }

    pub fn send_to_fut<'a>(&self, sock: SocketHandle, buf: &'a [u8], addr: impl ToSocketAddrs) -> impl Future<Output = io::Result<usize>> + 'a {
        let addr = addr
            .to_socket_addrs()
            .expect("Could not get address iterator")
            .next()
            .expect("Address iterator didn't provide any addresses");

        self.plat.send_to_fut(sock, buf, Some(addr))
    }
}