//! Declared build dependencies, immutable artifacts and revision-safe publication.
use super::store::{Retention, Store};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
};

pub use super::store::ContentKey;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRef {
    pub module: String,
    pub offset: usize,
    pub end: usize,
}

#[derive(Clone, Debug)]
pub struct Dependency {
    /// Semantic input name; identities may coalesce, but different input roles may not.
    pub role: String,
    pub node: String,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub id: String,
    pub implementation: String,
    pub inputs: Vec<u8>,
    pub dependencies: Vec<Dependency>,
    pub source: Option<SourceRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GraphError {
    pub message: String,
    pub chain: Vec<String>,
    pub source: Option<SourceRef>,
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if !self.chain.is_empty() {
            write!(f, " ({})", self.chain.join(" -> "))?;
        }
        Ok(())
    }
}
impl std::error::Error for GraphError {}

#[derive(Clone, Debug)]
pub struct Plan {
    nodes: BTreeMap<String, Node>,
    keys: BTreeMap<String, ContentKey>,
    order: Vec<String>,
    reverse: BTreeMap<String, BTreeSet<String>>,
    targets: Vec<String>,
}

const MAX_DEPTH: usize = 128;

struct PlannedNode<'a> {
    key: ContentKey,
    depth: usize,
    deepest_input: Option<&'a str>,
}

struct Traversal<'a> {
    nodes: &'a BTreeMap<String, Node>,
    products: BTreeMap<&'a str, PlannedNode<'a>>,
    order: Vec<&'a str>,
    reverse: BTreeMap<String, BTreeSet<String>>,
}
fn framed(hash: &mut Sha256, value: &[u8]) {
    hash.update((value.len() as u64).to_le_bytes());
    hash.update(value);
}

pub(crate) fn producer_key(implementation: &str, inputs: &[u8]) -> ContentKey {
    let mut hash = Sha256::new();
    framed(&mut hash, b"superspec-job-v1");
    framed(&mut hash, implementation.as_bytes());
    framed(&mut hash, inputs);
    hash.finalize().into()
}

impl Plan {
    /// Validates the entire supplied graph before running producers, including dormant nodes.
    pub fn new(nodes: Vec<Node>, targets: Vec<String>) -> Result<Self, GraphError> {
        let failure = |message: &str| GraphError {
            message: message.into(),
            chain: vec![],
            source: None,
        };
        if nodes.len() > 4096 || targets.len() > 4096 {
            return Err(failure("Build graph exceeds its node/target allowance."));
        }
        let mut by_id = BTreeMap::new();
        let mut bytes = targets.iter().map(String::len).sum::<usize>();
        let mut edges = 0usize;
        for node in nodes {
            edges = edges.saturating_add(node.dependencies.len());
            let references = node
                .dependencies
                .iter()
                .map(|dependency| dependency.role.len().saturating_add(dependency.node.len()))
                .sum::<usize>();
            bytes = bytes
                .checked_add(
                    node.inputs.len()
                        + node.id.len()
                        + node.implementation.len()
                        + references
                        + node.source.as_ref().map_or(0, |source| source.module.len()),
                )
                .ok_or_else(|| failure("Build graph input allowance overflow."))?;
            if bytes > 32 * 1024 * 1024
                || node.id.is_empty()
                || node.id.len() > 4096
                || node.implementation.is_empty()
                || node.dependencies.len() > 4096
                || edges > 65_536
                || node
                    .dependencies
                    .iter()
                    .any(|dependency| dependency.role.len() > 4096 || dependency.node.len() > 4096)
            {
                return Err(failure("Invalid or excessive build graph inputs."));
            }
            let id = node.id.clone();
            if by_id.insert(id.clone(), node).is_some() {
                return Err(failure(&format!("Duplicate build node {id}.")));
            }
        }
        let mut traversal = Traversal {
            nodes: &by_id,
            products: BTreeMap::new(),
            order: vec![],
            reverse: BTreeMap::new(),
        };
        let mut active = Vec::new();
        for id in by_id.keys() {
            traversal.visit(id, &mut active)?;
        }
        let needed = traversal.reachable_targets(&targets)?;
        let order = traversal
            .order
            .into_iter()
            .filter(|id| needed.contains(id))
            .map(str::to_owned)
            .collect();
        let keys = traversal
            .products
            .into_iter()
            .map(|(id, node)| (id.to_owned(), node.key))
            .collect();
        let reverse = traversal.reverse;
        Ok(Self {
            nodes: by_id,
            keys,
            order,
            reverse,
            targets,
        })
    }

    pub fn key(&self, id: &str) -> Option<ContentKey> {
        self.keys.get(id).copied()
    }

    /// Stable node IDs describe provenance; content keys determine whether artifacts remain valid.
    pub fn affected(&self, changed: impl IntoIterator<Item = String>) -> BTreeSet<String> {
        let mut result = BTreeSet::new();
        let mut pending: Vec<_> = changed.into_iter().collect();
        while let Some(id) = pending.pop() {
            if result.insert(id.clone()) {
                pending.extend(self.reverse.get(&id).into_iter().flatten().cloned());
            }
        }
        result
    }
}

impl<'a> Traversal<'a> {
    fn source(&self, id: &str) -> Option<SourceRef> {
        self.nodes.get(id).and_then(|node| node.source.clone())
    }

    fn visit(&mut self, id: &'a str, active: &mut Vec<&'a str>) -> Result<(), GraphError> {
        if let Some(product) = self.products.get(id) {
            if active.len() + product.depth > MAX_DEPTH {
                return Err(self.depth_error(active, id));
            }
            return Ok(());
        }
        if let Some(start) = active.iter().position(|name| *name == id) {
            let mut chain: Vec<_> = active[start..].iter().map(|id| (*id).into()).collect();
            chain.push(id.into());
            return Err(GraphError {
                message: "Cyclic build dependency.".into(),
                chain,
                source: active.last().and_then(|parent| self.source(parent)),
            });
        }
        if active.len() >= MAX_DEPTH {
            return Err(self.depth_error(active, id));
        }
        let node = self.nodes.get(id).ok_or_else(|| {
            let mut chain: Vec<_> = active.iter().map(|id| (*id).into()).collect();
            chain.push(id.into());
            GraphError {
                message: "Missing build dependency.".into(),
                chain,
                source: active.last().and_then(|parent| self.source(parent)),
            }
        })?;
        active.push(id);
        let mut dependencies: Vec<_> = node.dependencies.iter().collect();
        dependencies.sort_by(|a, b| a.role.cmp(&b.role));
        let mut previous_role = "";
        let mut hash = Sha256::new();
        framed(&mut hash, b"superspec-job-v1");
        framed(&mut hash, node.implementation.as_bytes());
        framed(&mut hash, &node.inputs);
        let mut depth = 1;
        let mut deepest_input = None;
        for dependency in dependencies {
            if dependency.role == previous_role {
                return Err(GraphError {
                    message: "Empty or duplicate dependency role.".into(),
                    chain: active.iter().map(|id| (*id).into()).collect(),
                    source: node.source.clone(),
                });
            }
            previous_role = &dependency.role;
            self.visit(&dependency.node, active)?;
            let input = &self.products[dependency.node.as_str()];
            framed(&mut hash, dependency.role.as_bytes());
            hash.update(input.key);
            if input.depth + 1 > depth {
                depth = input.depth + 1;
                deepest_input = Some(dependency.node.as_str());
            }
            self.reverse
                .entry(dependency.node.clone())
                .or_default()
                .insert(id.into());
        }
        active.pop();
        let key = hash.finalize().into();
        self.products.insert(
            id,
            PlannedNode {
                key,
                depth,
                deepest_input,
            },
        );
        self.order.push(id);
        Ok(())
    }

    fn depth_error(&self, active: &[&str], id: &str) -> GraphError {
        let mut chain: Vec<String> = active.iter().map(|id| (*id).into()).collect();
        let mut next = Some(id);
        while let Some(id) = next {
            chain.push(id.into());
            if chain.len() > MAX_DEPTH {
                break;
            }
            next = self.products[id].deepest_input;
        }
        let source = self.source(&chain[MAX_DEPTH - 1]);
        GraphError {
            message: format!("Build dependency depth exceeds {MAX_DEPTH}."),
            chain,
            source,
        }
    }

    fn reachable_targets<'t>(&self, targets: &'t [String]) -> Result<BTreeSet<&'t str>, GraphError>
    where
        'a: 't,
    {
        let mut pending: Vec<_> = targets.iter().rev().map(String::as_str).collect();
        let mut needed = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if needed.insert(id) {
                let node = self.nodes.get(id).ok_or_else(|| GraphError {
                    message: "Missing build target.".into(),
                    chain: vec![id.into()],
                    source: None,
                })?;
                pending.extend(
                    node.dependencies
                        .iter()
                        .rev()
                        .map(|input| input.node.as_str()),
                );
            }
        }
        Ok(needed)
    }
}

/// Shares completed artifacts across revisions. One producer at a time can allocate new artifacts.
pub struct Runtime<T> {
    latest: AtomicU64,
    generation: AtomicU64,
    producer: Mutex<()>,
    store: Mutex<Store<T>>,
    interests: Mutex<BTreeMap<(u64, u64, ContentKey), usize>>,
    retention: Retention,
}

thread_local! {
    static PRODUCER_STACK: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

/// Detect a callback that tries to start another producer while it owns the slot.
struct Producing;

impl Producing {
    fn enter(runtime: usize) -> Result<Self, String> {
        PRODUCER_STACK.with(|stack| {
            let mut stack = stack.borrow_mut();
            if stack.contains(&runtime) {
                return Err(
                    "A build producer cannot start another producer on its runtime.".into(),
                );
            }
            stack.push(runtime);
            Ok(Self)
        })
    }
}

impl Drop for Producing {
    fn drop(&mut self) {
        PRODUCER_STACK.with(|stack| {
            stack.borrow_mut().pop();
        });
    }
}

/// Queued consumers keep the producer they need alive even if its first consumer cancels.
struct Interests<'a> {
    registered: &'a Mutex<BTreeMap<(u64, u64, ContentKey), usize>>,
    generation: u64,
    revision: u64,
    keys: BTreeSet<ContentKey>,
}
impl Drop for Interests<'_> {
    fn drop(&mut self) {
        if let Ok(mut registered) = self.registered.lock() {
            for key in &self.keys {
                let identity = (self.generation, self.revision, *key);
                if let Some(count) = registered.get_mut(&identity) {
                    *count -= 1;
                    if *count == 0 {
                        registered.remove(&identity);
                    }
                }
            }
        }
    }
}

impl<T> Runtime<T> {
    pub fn new(maximum_bytes: usize) -> Self {
        Self {
            latest: AtomicU64::new(0),
            generation: AtomicU64::new(0),
            producer: Mutex::new(()),
            store: Mutex::new(Store::default()),
            interests: Mutex::new(BTreeMap::new()),
            retention: Retention {
                name: "authored-graph",
                bytes: maximum_bytes,
                entries: 4096,
            },
        }
    }

    pub(crate) fn lookup(&self, retention: Retention, key: &ContentKey) -> Option<Arc<T>> {
        self.store.lock().ok()?.get(retention, key)
    }

    /// Whether a planned node's artifact is retained, so running its plan would not produce it.
    pub(crate) fn holds(&self, key: &ContentKey) -> bool {
        self.store
            .lock()
            .is_ok_and(|mut store| store.get(self.retention, key).is_some())
    }

    pub(crate) fn retain(
        &self,
        retention: Retention,
        key: ContentKey,
        value: T,
        bytes: usize,
        replace: bool,
    ) {
        if let Ok(mut store) = self.store.lock() {
            store.insert(retention, key, Arc::new(value), bytes, replace);
        }
    }

    pub(crate) fn retained(&self, retention: Retention) -> (usize, usize) {
        self.store
            .lock()
            .map_or((0, 0), |store| store.stats(retention))
    }

    pub(crate) fn clear(&self) {
        let previous = self.store.lock().ok().map(|mut store| {
            self.generation.fetch_add(1, Ordering::AcqRel);
            std::mem::take(&mut *store)
        });
        drop(previous);
    }

    pub fn begin(&self, revision: u64) -> Result<(), String> {
        let previous = self.latest.fetch_max(revision, Ordering::AcqRel);
        if revision < previous {
            Err("Build revision is obsolete.".into())
        } else {
            Ok(())
        }
    }

    pub fn current(&self, revision: u64) -> bool {
        self.latest.load(Ordering::Acquire) == revision
    }

    pub fn run(
        &self,
        revision: u64,
        plan: &Plan,
        cancelled: &dyn Fn() -> bool,
        mut produce: impl FnMut(
            &Node,
            &BTreeMap<String, Arc<T>>,
            &dyn Fn() -> bool,
        ) -> Result<(T, usize), String>,
    ) -> Result<BTreeMap<String, Arc<T>>, String> {
        let generation = self.generation.load(Ordering::Acquire);
        self.begin(revision)?;
        let reset = || self.generation.load(Ordering::Acquire) != generation;
        let stopped = || reset() || cancelled() || !self.current(revision);
        let interests = Interests {
            registered: &self.interests,
            generation,
            revision,
            keys: plan.order.iter().map(|id| plan.keys[id]).collect(),
        };
        {
            let mut registered = self
                .interests
                .lock()
                .map_err(|_| "Build consumers are unavailable.")?;
            for key in &interests.keys {
                *registered.entry((generation, revision, *key)).or_default() += 1;
            }
        }
        let mut complete = BTreeMap::<String, Arc<T>>::new();
        let mut coalesced = BTreeMap::<ContentKey, Arc<T>>::new();
        let find = |key: &ContentKey| {
            self.store
                .lock()
                .map(|mut store| store.get(self.retention, key))
                .map_err(|_| "Build artifact store is unavailable.")
        };
        for id in &plan.order {
            if stopped() {
                return Err("Build cancelled or superseded.".into());
            }
            let key = plan.keys[id];
            let value = if let Some(value) = find(&key)?.or_else(|| coalesced.get(&key).cloned()) {
                value
            } else {
                // Admission and eviction stay available during production. Recheck after
                // waiting for the producer slot because another consumer may have filled it.
                let _producing = Producing::enter(std::ptr::from_ref(self).addr())?;
                let _producer = self
                    .producer
                    .lock()
                    .map_err(|_| "Build producer is unavailable.")?;
                if stopped() {
                    return Err("Build cancelled or superseded.".into());
                }
                if let Some(value) = find(&key)? {
                    coalesced.insert(key, value.clone());
                    complete.insert(id.clone(), value);
                    continue;
                }
                let node = &plan.nodes[id];
                let inputs = node
                    .dependencies
                    .iter()
                    .map(|dependency| (dependency.role.clone(), complete[&dependency.node].clone()))
                    .collect();
                let producer_stopped = || {
                    if reset() {
                        return true;
                    }
                    if !stopped() {
                        return false;
                    }
                    let latest = self.latest.load(Ordering::Acquire);
                    self.interests.lock().map_or(true, |registered| {
                        registered
                            .get(&(generation, latest, key))
                            .copied()
                            .unwrap_or(0)
                            <= usize::from(latest == revision)
                    })
                };
                let (value, bytes) = produce(node, &inputs, &producer_stopped)?;
                if producer_stopped() {
                    return Err("Build cancelled or superseded.".into());
                }
                let value = Arc::new(value);
                let mut store = self
                    .store
                    .lock()
                    .map_err(|_| "Build artifact store is unavailable.")?;
                if reset() {
                    return Err("Build cancelled or superseded.".into());
                }
                store.insert(self.retention, key, value.clone(), bytes, false);
                value
            };
            coalesced.insert(key, value.clone());
            complete.insert(id.clone(), value);
        }
        if stopped() {
            return Err("Build cancelled or superseded.".into());
        }
        Ok(plan
            .targets
            .iter()
            .map(|id| (id.clone(), complete[id].clone()))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, input: &[u8], dependencies: &[(&str, &str)]) -> Node {
        Node {
            id: id.into(),
            implementation: "fixture-v1".into(),
            inputs: input.into(),
            dependencies: dependencies
                .iter()
                .map(|(role, node)| Dependency {
                    role: (*role).into(),
                    node: (*node).into(),
                })
                .collect(),
            source: Some(SourceRef {
                module: "fixture.ms".into(),
                offset: 4,
                end: 9,
            }),
        }
    }

    #[test]
    fn cycles_and_missing_imports_retain_reference_chain() {
        let error = Plan::new(
            vec![
                node("a", b"", &[("source", "b")]),
                node("b", b"", &[("source", "a")]),
            ],
            vec!["a".into()],
        )
        .unwrap_err();
        assert_eq!(error.chain, ["a", "b", "a"]);
        assert_eq!(error.source.unwrap().module, "fixture.ms");
        let error = Plan::new(
            vec![node("a", b"", &[("texture", "missing")])],
            vec!["a".into()],
        )
        .unwrap_err();
        assert_eq!(error.chain, ["a", "missing"]);
    }

    #[test]
    fn depth_limit_is_independent_of_root_names_and_requested_targets() {
        let chain = |length: usize, reverse_ids: bool| {
            let ids: Vec<_> = (0..length)
                .map(|i| format!("{:03}", if reverse_ids { length - i } else { i }))
                .collect();
            let nodes = ids
                .iter()
                .enumerate()
                .map(|(i, id)| {
                    let mut value = node(id, b"", &[]);
                    value
                        .dependencies
                        .extend(ids.get(i + 1).map(|input| Dependency {
                            role: "input".into(),
                            node: input.clone(),
                        }));
                    value.source.as_mut().unwrap().offset = i;
                    value
                })
                .collect();
            (nodes, ids)
        };
        for reverse_ids in [false, true] {
            let (nodes, ids) = chain(128, reverse_ids);
            assert!(
                Plan::new(nodes, vec![ids[0].clone()])
                    .unwrap()
                    .key(&ids[0])
                    .is_some()
            );
            let (nodes, ids) = chain(129, reverse_ids);
            let error = Plan::new(nodes, vec![]).unwrap_err();
            assert_eq!(error.chain, ids);
            assert_eq!(error.source.unwrap().offset, 127);
        }
    }

    #[test]
    fn material_edit_preserves_geometry_and_equal_requests_share_work() {
        let graph = |material: &[u8]| {
            Plan::new(
                vec![
                    node("geometry", b"pipe", &[]),
                    node("material", material, &[]),
                    node(
                        "left",
                        b"binding",
                        &[("mesh", "geometry"), ("finish", "material")],
                    ),
                    node(
                        "right",
                        b"binding",
                        &[("mesh", "geometry"), ("finish", "material")],
                    ),
                ],
                vec!["geometry".into(), "left".into(), "right".into()],
            )
            .unwrap()
        };
        let first = graph(b"red");
        let changed = graph(b"blue");
        assert_eq!(first.key("geometry"), changed.key("geometry"));
        assert_ne!(first.key("left"), changed.key("left"));
        assert_eq!(
            first.affected(["material".into()]),
            BTreeSet::from(["material".into(), "left".into(), "right".into()])
        );
        let runtime = Runtime::new(1024);
        let mut calls = vec![];
        let output = runtime
            .run(1, &first, &|| false, |n, _, _| {
                calls.push(n.id.clone());
                Ok((n.inputs.clone(), n.inputs.len()))
            })
            .unwrap();
        assert_eq!(calls.len(), 3);
        assert!(Arc::ptr_eq(&output["left"], &output["right"]));
        let next = runtime
            .run(2, &changed, &|| false, |n, _, _| {
                Ok((n.inputs.clone(), n.inputs.len()))
            })
            .unwrap();
        assert!(Arc::ptr_eq(&output["geometry"], &next["geometry"]));
    }

    #[test]
    fn failed_and_obsolete_results_never_publish_and_can_recover() {
        let graph = Plan::new(
            vec![node("material", b"source", &[])],
            vec!["material".into()],
        )
        .unwrap();
        let runtime = Runtime::<Vec<u8>>::new(1024);
        assert!(
            runtime
                .run(1, &graph, &|| false, |_, _, _| Err("source error".into()))
                .is_err()
        );
        assert!(
            runtime
                .run(2, &graph, &|| false, |_, _, _| {
                    runtime.begin(3)?;
                    Ok((vec![1], 1))
                })
                .is_err()
        );
        let result = runtime
            .run(3, &graph, &|| false, |_, _, _| Ok((vec![3], 1)))
            .unwrap();
        assert_eq!(*result["material"], vec![3]);
        assert!(
            runtime
                .run(2, &graph, &|| false, |_, _, _| unreachable!())
                .is_err()
        );
    }

    #[test]
    fn concurrent_consumer_survives_first_consumers_cancellation() {
        use std::sync::{Once, atomic::AtomicBool, mpsc};
        let runtime = Arc::new(Runtime::<u32>::new(1024));
        let plan = Arc::new(
            Plan::new(vec![node("shared", b"image", &[])], vec!["shared".into()]).unwrap(),
        );
        let cancelled = Arc::new(AtomicBool::new(false));
        let (started_tx, started_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        let first = {
            let runtime = runtime.clone();
            let plan = plan.clone();
            let cancelled = cancelled.clone();
            std::thread::spawn(move || {
                runtime.run(
                    1,
                    &plan,
                    &|| cancelled.load(Ordering::Acquire),
                    |_, _, stopped| {
                        started_tx.send(()).unwrap();
                        resume_rx.recv().unwrap();
                        assert!(!stopped(), "the second consumer still needs this artifact");
                        Ok((42, 4))
                    },
                )
            })
        };
        started_rx.recv().unwrap();
        let (joined_tx, joined_rx) = mpsc::channel();
        let second = {
            let runtime = runtime.clone();
            let plan = plan.clone();
            std::thread::spawn(move || {
                let once = Once::new();
                runtime.run(
                    1,
                    &plan,
                    &|| {
                        once.call_once(|| joined_tx.send(()).unwrap());
                        false
                    },
                    |_, _, _| panic!("concurrent request must reuse the in-flight producer"),
                )
            })
        };
        joined_rx.recv().unwrap();
        cancelled.store(true, Ordering::Release);
        resume_tx.send(()).unwrap();
        assert!(
            first.join().unwrap().is_err(),
            "cancelled consumer must not publish"
        );
        assert_eq!(*second.join().unwrap().unwrap()["shared"], 42);
        assert!(runtime.interests.lock().unwrap().is_empty());
    }

    #[test]
    fn producer_can_read_the_store_and_reset_cancels_its_publication() {
        let (done, completed) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            check_store_reentrancy();
            done.send(()).unwrap();
        });
        completed
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("artifact producer and cancellation callbacks must be able to use the store");
        worker.join().unwrap();
    }

    fn check_store_reentrancy() {
        let plan = Plan::new(vec![node("value", b"", &[])], vec!["value".into()]).unwrap();
        let key = plan.key("value").unwrap();
        let runtime = Runtime::new(1024);
        assert!(
            runtime
                .run(1, &plan, &|| false, |_, _, stopped| {
                    assert!(!runtime.holds(&key));
                    let error = runtime
                        .run(1, &plan, &|| false, |_, _, _| unreachable!())
                        .unwrap_err();
                    assert!(error.contains("another producer"));
                    runtime.clear();
                    assert!(stopped());
                    Ok((1, 1))
                })
                .is_err()
        );
        assert!(!runtime.holds(&key));
        let request = runtime.run(1, &plan, &|| runtime.holds(&key), |_, _, _| {
            assert_eq!(runtime.retained(runtime.retention), (0, 0));
            Ok((2, 1))
        });
        assert!(
            request.is_err(),
            "the consumer cancelled when its artifact appeared"
        );
        let result = runtime
            .run(1, &plan, &|| false, |_, _, _| panic!("published once"))
            .unwrap();
        assert_eq!(*result["value"], 2);
    }

    #[test]
    fn eviction_and_changed_dependency_edges_rebuild_only_needed_work() {
        let graph = |source: &[u8], imports: bool| {
            Plan::new(
                vec![
                    node("image", source, &[]),
                    node(
                        "output",
                        b"material",
                        if imports { &[("image", "image")] } else { &[] },
                    ),
                ],
                vec!["output".into()],
            )
            .unwrap()
        };
        let runtime = Runtime::<Vec<u8>>::new(1);
        let first = graph(b"a", true);
        let second = graph(b"b", false);
        assert_ne!(first.key("output"), second.key("output"));
        assert_eq!(
            second.affected(["image".into()]),
            BTreeSet::from(["image".into()])
        );
        let mut produced = Vec::new();
        for (revision, plan) in [(1, &first), (2, &second), (3, &first)] {
            runtime
                .run(revision, plan, &|| false, |node, _, _| {
                    produced.push(node.id.clone());
                    Ok((node.inputs.clone(), 1))
                })
                .unwrap();
        }
        assert_eq!(produced, ["image", "output", "output", "image", "output"]);
    }
}
