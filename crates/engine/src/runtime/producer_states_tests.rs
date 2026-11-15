use super::producer_states::ProducerStates;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct RasterBookkeeping {
    visits: AtomicUsize,
    dropped: Arc<AtomicUsize>,
}
impl Drop for RasterBookkeeping {
    fn drop(&mut self) {
        self.dropped
            .store(self.dropped.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
    }
}
struct LayoutBookkeeping(AtomicUsize);

#[test]
fn composition_supplies_lazy_isolated_state_and_clear_releases_it() {
    let states = ProducerStates::default();
    let dropped = Arc::new(AtomicUsize::new(0));
    assert!(states.inspect::<RasterBookkeeping, _>(|_| ()).is_none());
    assert!(states.inspect::<LayoutBookkeeping, _>(|_| ()).is_none());
    let make_raster = || RasterBookkeeping {
        visits: AtomicUsize::new(0),
        dropped: dropped.clone(),
    };
    states.with(make_raster, |raster| {
        raster.visits.store(7, Ordering::Relaxed)
    });
    states.with(
        || LayoutBookkeeping(AtomicUsize::new(3)),
        |layout| {
            // Entering another producer while a state is in use must not borrow-panic.
            assert_eq!(
                states
                    .inspect::<RasterBookkeeping, _>(|state| state.visits.load(Ordering::Relaxed)),
                Some(7)
            );
            layout.0.store(11, Ordering::Relaxed);
        },
    );
    states.with::<RasterBookkeeping, _>(
        || panic!("initialized producer"),
        |state| {
            assert_eq!(state.visits.load(Ordering::Relaxed), 7);
        },
    );
    assert_eq!(
        states.inspect::<LayoutBookkeeping, _>(|state| state.0.load(Ordering::Relaxed)),
        Some(11)
    );
    states.clear();
    assert_eq!(dropped.load(Ordering::Relaxed), 1);
    assert!(states.inspect::<LayoutBookkeeping, _>(|_| ()).is_none());
    states.with(make_raster, |state| {
        assert_eq!(state.visits.load(Ordering::Relaxed), 0)
    });
    states.clear();
    assert_eq!(dropped.load(Ordering::Relaxed), 2);
}

#[test]
fn failed_composition_setup_is_not_retained_and_active_state_can_unwind_after_clear() {
    let states = ProducerStates::default();
    let failed: Result<(), &str> = states.try_with::<LayoutBookkeeping, _, _>(
        || Err("missing producer input"),
        |_| panic!("failed setup cannot run"),
    );
    assert_eq!(failed, Err("missing producer input"));
    assert!(states.inspect::<LayoutBookkeeping, _>(|_| ()).is_none());
    states
        .try_with(
            || Ok::<_, &str>(LayoutBookkeeping(AtomicUsize::new(5))),
            |state| {
                assert_eq!(state.0.load(Ordering::Relaxed), 5);
            },
        )
        .unwrap();

    let dropped = Arc::new(AtomicUsize::new(0));
    states.with(
        || RasterBookkeeping {
            visits: AtomicUsize::new(4),
            dropped: dropped.clone(),
        },
        |state| {
            states.clear();
            assert_eq!(
                dropped.load(Ordering::Relaxed),
                0,
                "an active build still borrows its state"
            );
            assert!(states.inspect::<RasterBookkeeping, _>(|_| ()).is_none());
            assert_eq!(state.visits.load(Ordering::Relaxed), 4);
        },
    );
    assert_eq!(
        dropped.load(Ordering::Relaxed),
        1,
        "cleared state is released when its active call finishes"
    );
}
