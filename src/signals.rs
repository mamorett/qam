use std::sync::OnceLock;
use tokio::sync::watch;

static SHUTDOWN_WATCH: OnceLock<(watch::Sender<bool>, watch::Receiver<bool>)> = OnceLock::new();

/// Initializes the global signal handlers for SIGINT (Ctrl-C) and SIGTERM.
/// Must be called early in main / run_cli so the OS registers the signal handler.
pub fn init_signals() {
    if SHUTDOWN_WATCH.get().is_some() {
        return;
    }

    #[cfg(unix)]
    use tokio::signal::unix::{SignalKind, signal};
    #[cfg(unix)]
    let mut sigint = signal(SignalKind::interrupt()).ok();
    #[cfg(unix)]
    let mut sigterm = signal(SignalKind::terminate()).ok();

    let (tx, rx) = watch::channel(false);
    let _ = SHUTDOWN_WATCH.set((tx, rx));

    tokio::spawn(async move {
        #[cfg(unix)]
        {
            match (sigint.as_mut(), sigterm.as_mut()) {
                (Some(si), Some(st)) => {
                    tokio::select! {
                        _ = si.recv() => {},
                        _ = st.recv() => {},
                    }
                }
                (Some(si), None) => {
                    let _ = si.recv().await;
                }
                (None, Some(st)) => {
                    let _ = st.recv().await;
                }
                (None, None) => {
                    let _ = tokio::signal::ctrl_c().await;
                }
            }

            if let Some((tx, _)) = SHUTDOWN_WATCH.get() {
                let _ = tx.send(true);
            }

            std::mem::forget(sigint);
            std::mem::forget(sigterm);
        }

        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
            if let Some((tx, _)) = SHUTDOWN_WATCH.get() {
                let _ = tx.send(true);
            }
        }
    });
}

/// A future that completes when a shutdown signal (Ctrl-C / SIGINT, or SIGTERM on Unix) is received.
pub async fn wait_for_shutdown() {
    if let Some((_, rx)) = SHUTDOWN_WATCH.get() {
        let mut rx = rx.clone();
        if *rx.borrow() {
            return;
        }
        while rx.changed().await.is_ok() {
            if *rx.borrow() {
                return;
            }
        }
    } else {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            let mut sigterm = match signal(SignalKind::terminate()) {
                Ok(s) => s,
                Err(_) => {
                    let _ = tokio::signal::ctrl_c().await;
                    return;
                }
            };
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {},
                _ = sigterm.recv() => {},
            }
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
        }
    }
}
