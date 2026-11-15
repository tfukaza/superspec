use super::Sources;
use std::cell::Cell;

#[test]
fn source_lifetime_is_one_revision_and_failed_reads_do_not_become_entries() {
    let requests = Cell::new(0);
    let available = Cell::new(false);
    let loader = |name: &str| {
        requests.set(requests.get() + 1);
        assert_eq!(name, "textures/stone.png");
        available
            .get()
            .then(|| vec![requests.get() as u8])
            .ok_or("missing".into())
    };
    let first = Sources::new(&loader, 16);
    assert_eq!(first.read("textures/stone.png").unwrap_err(), "missing");
    available.set(true);
    let bytes = first.read("textures/stone.png").unwrap();
    assert_eq!(&*bytes, &[2]);
    assert!(std::sync::Arc::ptr_eq(
        &bytes,
        &first.read("textures/stone.png").unwrap()
    ));
    assert_eq!(requests.get(), 2);
    let second = Sources::new(&loader, 16);
    assert_eq!(&*second.read("textures/stone.png").unwrap(), &[3]);
    let uncached = Sources::new(&loader, 0);
    uncached.read("textures/stone.png").unwrap();
    uncached.read("textures/stone.png").unwrap();
    assert_eq!(requests.get(), 5);
}
