use scafra_foundation::{
    capture_error_backtrace, init_runtime_logging, BacktraceMode, LogLevel, LoggingConfig,
};
use std::{sync::Arc, thread};

#[test]
fn concurrent_initialization_is_safe_and_keeps_one_policy() {
    const THREADS: usize = 8;

    let barrier = Arc::new(std::sync::Barrier::new(THREADS));
    let workers = (0..THREADS)
        .map(|index| {
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                init_runtime_logging(&LoggingConfig {
                    level: LogLevel::Off,
                    backtrace: if index % 2 == 0 {
                        BacktraceMode::Off
                    } else {
                        BacktraceMode::Errors
                    },
                });
            })
        })
        .collect::<Vec<_>>();

    for worker in workers {
        worker
            .join()
            .expect("concurrent runtime initialization must not panic");
    }

    let enabled = capture_error_backtrace().is_some();
    init_runtime_logging(&LoggingConfig {
        level: LogLevel::Off,
        backtrace: if enabled {
            BacktraceMode::Off
        } else {
            BacktraceMode::Errors
        },
    });
    assert_eq!(capture_error_backtrace().is_some(), enabled);
}
