use tokio::task::JoinHandle;

pub(crate) struct TaskHandle<O = ()> {
    inner: JoinHandle<O>,
}

impl TaskHandle {
    pub(crate) async fn shutdown(self) {
        self.inner.abort();
    }
}

pub(crate) fn spawn<F, O>(task: F) -> TaskHandle<O>
where
    F: Future<Output = O> + Send + 'static,
    O: Send + 'static,
{
    TaskHandle {
        inner: tokio::spawn(task),
    }
}
