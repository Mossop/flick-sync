use async_std::task::{self, JoinHandle};

pub(crate) struct TaskHandle<O = ()> {
    inner: JoinHandle<O>,
}

impl TaskHandle {
    pub(crate) async fn shutdown(self) {
        self.inner.cancel().await;
    }
}

pub(crate) fn spawn<F, O>(task: F) -> TaskHandle<O>
where
    F: Future<Output = O> + Send + 'static,
    O: Send + 'static,
{
    TaskHandle {
        inner: task::spawn(task),
    }
}
