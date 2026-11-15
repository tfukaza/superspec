//! Index-only worker dispatch; producers own admission, budgets and publication.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) type Joined<T> = Vec<std::thread::Result<Vec<(usize, Result<T, String>)>>>;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn selected<T: Send>(
    indices: &[usize],
    workers: usize,
    operation: &(impl Fn(usize) -> Result<T, String> + Sync),
) -> Joined<T> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let next = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        let mut handles = vec![];
        for _ in 0..workers {
            let next = &next;
            handles.push(scope.spawn(move || {
                let mut result = vec![];
                while let Some(&i) = indices.get(next.fetch_add(1, Ordering::Relaxed)) {
                    result.push((i, operation(i)));
                }
                result
            }));
        }
        handles.into_iter().map(|h| h.join()).collect::<Vec<_>>()
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn named<T: Send>(
    count: usize,
    workers: usize,
    prefix: &str,
    operation: &(impl Fn(usize) -> Result<T, String> + Sync),
) -> (Joined<T>, Option<String>) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let next = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        let mut start_error = None;
        for worker in 0..workers {
            let started = std::thread::Builder::new()
                .name(format!("{prefix}-{worker}"))
                .spawn_scoped(scope, || {
                    let mut results = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= count {
                            break;
                        }
                        results.push((i, operation(i)));
                    }
                    results
                });
            match started {
                Ok(handle) => handles.push(handle),
                Err(error) => {
                    start_error = Some(error.to_string());
                    break;
                }
            }
        }
        // Every started worker joins before the caller observes a startup or operation failure.
        (
            handles.into_iter().map(|h| h.join()).collect::<Vec<_>>(),
            start_error,
        )
    })
}

#[cfg(all(target_arch = "wasm32", feature = "wasm-threads"))]
pub(crate) fn lanes<T: Send>(
    indices: &[usize],
    lanes: usize,
    operation: &(impl Fn(usize) -> Result<T, String> + Sync),
) -> Vec<Vec<(usize, Result<T, String>)>> {
    use rayon::prelude::*;
    (0..lanes)
        .into_par_iter()
        .map(|lane| {
            (lane..indices.len())
                .step_by(lanes)
                .map(|n| {
                    let i = indices[n];
                    (i, operation(i))
                })
                .collect::<Vec<_>>()
        })
        .collect()
}
