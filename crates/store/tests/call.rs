//! `Store::call` when the runtime that runs its blocking task goes away.

use std::future::Future;
use std::task::Poll;

use harness_store::Store;

/// A call made while its runtime shuts down (its blocking task is cancelled
/// before it starts) returns an error. It panicked before (review A21-9:
/// `JoinError::into_panic` on a cancelled task).
#[test]
fn a_call_cancelled_at_shutdown_is_an_error_not_a_panic() {
    let store = Store::open_in_memory().unwrap();
    let a = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let dying = a.handle().clone();
    a.shutdown_background();
    let b = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let res = b.block_on(async {
        let mut call = Box::pin(store.call(|s| s.audit_count()));
        {
            // The blocking task goes to the runtime that is gone.
            let _in_a = dying.enter();
            std::future::poll_fn(|cx| {
                let _ = call.as_mut().poll(cx);
                Poll::Ready(())
            })
            .await;
        }
        call.await
    });
    assert!(res.is_err(), "{res:?}");
}
