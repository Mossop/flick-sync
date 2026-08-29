mod mutex;
mod opmutex;
mod rwlock;

const MAX_LOCK_TIME: Duration = Duration::from_millis(1000);
const MAX_WAIT_TIME: Duration = Duration::from_millis(200);

use std::time::Duration;

pub(crate) use mutex::*;
pub use opmutex::*;
pub(crate) use rwlock::*;
