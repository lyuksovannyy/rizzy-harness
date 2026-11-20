//! The one log capture of this test program. Every test in it that asserts
//! "this secret is in no log line" reads the same buffer, so a negative
//! assertion also covers the lines the other tests' servers wrote. The
//! subscriber can be installed once per process, which is why these files
//! share a program and why a file that asserts "nothing was logged" or a
//! line count cannot join.

use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};

/// Everything the process logs, at every level, unfiltered.
pub fn logs() -> Arc<Mutex<Vec<u8>>> {
    static BUF: OnceLock<Arc<Mutex<Vec<u8>>>> = OnceLock::new();
    BUF.get_or_init(|| {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let sink = buf.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(move || Sink(sink.clone()))
            .finish();
        tracing::subscriber::set_global_default(subscriber).expect("one subscriber");
        buf
    })
    .clone()
}

struct Sink(Arc<Mutex<Vec<u8>>>);

impl Write for Sink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
