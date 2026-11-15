use super::batch::Batch;
use std::cell::RefCell;

#[test]
fn graph_and_external_producers_share_retention_without_crossing_partition_budgets() {
    use super::{
        dependencies::{Node, Plan, Runtime, producer_key},
        store::Retention,
    };
    let runtime = Runtime::new(16);
    let raster = Retention {
        name: "raster-fixture",
        bytes: 9,
        entries: 2,
    };
    let binding = Retention {
        name: "binding-fixture",
        bytes: 20,
        entries: 1,
    };
    let key = producer_key("fixture-v1", b"one");
    let plan = Plan::new(
        vec![Node {
            id: "one".into(),
            implementation: "fixture-v1".into(),
            inputs: b"one".to_vec(),
            dependencies: vec![],
            source: None,
        }],
        vec!["one".into()],
    )
    .unwrap();
    assert_eq!(plan.key("one"), Some(key));
    runtime.retain(raster, key, "raster", 9, false);
    runtime.retain(raster, key, "ignored duplicate", 9, false);
    assert_eq!(*runtime.lookup(raster, &key).unwrap(), "raster");
    let graph = runtime
        .run(1, &plan, &|| false, |_, _, _| Ok(("graph", 16)))
        .unwrap();
    assert_eq!(*graph["one"], "graph");
    assert_eq!(runtime.retained(raster), (1, 9));
    runtime.retain(binding, key, "first binding", 12, true);
    let retained = runtime.lookup(binding, &key).unwrap();
    runtime.retain(binding, key, "replacement", 16, true);
    assert_eq!(*retained, "first binding");
    assert_eq!(*runtime.lookup(binding, &key).unwrap(), "replacement");
    runtime.retain(binding, key, "oversized", 21, true);
    assert_eq!(*runtime.lookup(binding, &key).unwrap(), "replacement");
    let other = producer_key("fixture-v1", b"two");
    runtime.retain(raster, other, "second raster", 9, false);
    assert!(runtime.lookup(raster, &key).is_none());
    assert_eq!(runtime.retained(binding), (1, 16));
    let warm = runtime
        .run(2, &plan, &|| false, |_, _, _| {
            panic!("other partitions must not evict graph artifacts")
        })
        .unwrap();
    assert!(std::sync::Arc::ptr_eq(&graph["one"], &warm["one"]));
    runtime.clear();
    assert_eq!(runtime.retained(raster), (0, 0));
    assert_eq!(runtime.retained(binding), (0, 0));
    let rebuilt = runtime
        .run(3, &plan, &|| false, |_, _, _| Ok(("rebuilt", 7)))
        .unwrap();
    assert_eq!(*rebuilt["one"], "rebuilt");
}

#[test]
fn ordered_batch_reuses_completed_values_and_groups_only_equal_missing_keys() {
    let probes = RefCell::new(vec![]);
    let batch = Batch::prepare(
        vec![
            b"ready".to_vec(),
            b"new".to_vec(),
            b"new".to_vec(),
            b"other".to_vec(),
        ],
        |index, key| {
            probes.borrow_mut().push(index);
            (key == b"ready").then_some("completed")
        },
        &|| false,
        "cancelled",
    )
    .unwrap();
    assert_eq!(*probes.borrow(), [0, 1, 2, 3]);
    assert_eq!(batch.ordered, [Some("completed"), None, None, None]);
    assert_eq!(batch.missing(), [1, 3]);
    assert_eq!(batch.shared, [(2, 1)]);
    assert!(
        Batch::<()>::prepare(
            vec![vec![1]],
            |_, _| panic!("cancelled lookup"),
            &|| true,
            "cancelled"
        )
        .is_err()
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn generic_dispatch_preserves_selected_indices_and_joins_every_worker() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let calls = AtomicUsize::new(0);
    let results = super::dispatch::selected(&[7, 2, 9], 2, &|i| {
        calls.fetch_add(1, Ordering::Relaxed);
        Ok(i * 3)
    });
    let mut actual = results
        .into_iter()
        .flat_map(|joined| joined.unwrap())
        .map(|(i, result)| (i, result.unwrap()))
        .collect::<Vec<_>>();
    actual.sort();
    assert_eq!(actual, [(2, 6), (7, 21), (9, 27)]);
    assert_eq!(calls.load(Ordering::Relaxed), 3);
}
