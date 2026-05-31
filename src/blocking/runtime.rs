use std::future::Future;
use std::sync::OnceLock;
use tokio::runtime::{Handle, Runtime};

use crate::error::HonchoError;

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

/// Returns worker count: min(available parallelism, 8), default 4.
fn worker_count() -> usize {
    std::thread::available_parallelism().map_or_else(|_| 4, |n| n.get().min(8))
}

#[expect(clippy::expect_used)]
fn get_or_create_runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(worker_count())
            .enable_all()
            .build()
            .expect("failed to create honcho-ai blocking runtime")
    })
}

pub(crate) fn handle() -> Handle {
    get_or_create_runtime().handle().clone()
}

pub(crate) fn block_on<F: Future>(future: F) -> crate::error::Result<F::Output> {
    match Handle::try_current() {
        Ok(_) => Err(HonchoError::Configuration(
            "blocking::Honcho cannot be called from within an async runtime. \
             Use the async Honcho client instead."
                .to_string(),
        )),
        Err(_) => Ok(get_or_create_runtime().block_on(future)),
    }
}
