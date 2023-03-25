use std::sync::Arc;

use crate::Inner;

pub struct Server {
    pub(crate) id: String,
    pub(crate) inner: Arc<Inner>,
}
