//! Declared build dependencies, immutable artifacts and revision-safe publication.
use super::store::{Retention, Store};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
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
        let mut plan = Self {
            nodes: BTreeMap::new(),
            keys: BTreeMap::new(),
            order: vec![],
            reverse: BTreeMap::new(),
            targets,
        };
        let mut bytes = plan.targets.iter().map(String::len).sum::<usize>();
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
            if plan.nodes.insert(id.clone(), node).is_some() {
                return Err(failure(&format!("Duplicate build node {id}.")));
            }
        }
        let ids: Vec<_> = plan.nodes.keys().cloned().collect();
        for id in ids {
            plan.visit(&id, &mut Vec::new())?;
        }
        let mut needed = BTreeSet::new();
        for target in &plan.targets {
            plan.collect(target, &mut needed)?;
        }
        plan.order.retain(|id| needed.contains(id));
        Ok(plan)
    }

    fn visit(&mut self, id: &str, active: &mut Vec<String>) -> Result<ContentKey, GraphError> {
        if let Some(key) = self.keys.get(id) {
            return Ok(*key);
        }
        let source = active
            .last()
            .and_then(|parent| self.nodes.get(parent))
            .and_then(|n| n.source.clone());
        if let Some(start) = active.iter().position(|name| name == id) {
            let mut chain = active[start..].to_vec();
            chain.push(id.into());
            return Err(GraphError {
                message: "Cyclic build dependency.".into(),
                chain,
                source,
            });
        }
        if active.len() >= 128 {
            return Err(GraphError {
                message: "Build dependency depth exceeds 128.".into(),
                chain: active.clone(),
                source,
            });
        }
        let node = self.nodes.get(id).cloned().ok_or_else(|| {
            let mut chain = active.clone();
            chain.push(id.into());
            GraphError {
                message: "Missing build dependency.".into(),
                chain,
                source,
            }
        })?;
        active.push(id.into());
        let mut dependencies = node.dependencies;
        dependencies.sort_by(|a, b| a.role.cmp(&b.role));
        let mut roles = BTreeSet::new();
        let mut hash = Sha256::new();
        framed(&mut hash, b"superspec-job-v1");
        framed(&mut hash, node.implementation.as_bytes());
        framed(&mut hash, &node.inputs);
        for dependency in dependencies {
            if dependency.role.is_empty() || !roles.insert(dependency.role.clone()) {
                return Err(GraphError {
                    message: "Empty or duplicate dependency role.".into(),
                    chain: active.clone(),
                    source: node.source,
                });
            }
            let key = self.visit(&dependency.node, active)?;
            framed(&mut hash, dependency.role.as_bytes());
            hash.update(key);
            self.reverse
                .entry(dependency.node)
                .or_default()
                .insert(id.into());
        }
        active.pop();
        let key = hash.finalize().into();
        self.keys.insert(id.into(), key);
        self.order.push(id.into());
        Ok(key)
    }

    fn collect(&self, id: &str, needed: &mut BTreeSet<String>) -> Result<(), GraphError> {
        let node = self.nodes.get(id).ok_or_else(|| GraphError {
            message: "Missing build target.".into(),
            chain: vec![id.into()],
            source: None,
        })?;
        if needed.insert(id.into()) {
            for dependency in &node.dependencies {
                self.collect(&dependency.node, needed)?;
            }
        }
        Ok(())
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

/// Serializes producers within a session so simultaneous equal requests share completed work.
/// Starting a newer revision is lock-free, allowing obsolete producers to observe cancellation.
pub struct Runtime<T> {
    latest: AtomicU64,
    store: Mutex<Store<T>>,
    interests: Mutex<BTreeMap<(u64, ContentKey), usize>>,
    retention: Retention,
}

/// Register before waiting for the producer lock, so cancelling its first consumer cannot
/// cancel work still needed by a queued consumer of the current revision.
struct Interests<'a> {
    registered: &'a Mutex<BTreeMap<(u64, ContentKey), usize>>,
    revision: u64,
    keys: BTreeSet<ContentKey>,
}
impl Drop for Interests<'_> {
    fn drop(&mut self) {
        if let Ok(mut registered) = self.registered.lock() {
            for key in &self.keys {
                let identity = (self.revision, *key);
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
        if let Ok(mut store) = self.store.lock() {
            *store = Store::default();
        }
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
        self.begin(revision)?;
        let stopped = || cancelled() || !self.current(revision);
        let interests = Interests {
            registered: &self.interests,
            revision,
            keys: plan.order.iter().map(|id| plan.keys[id]).collect(),
        };
        {
            let mut registered = self
                .interests
                .lock()
                .map_err(|_| "Build consumers are unavailable.")?;
            for key in &interests.keys {
                *registered.entry((revision, *key)).or_default() += 1;
            }
        }
        if stopped() {
            return Err("Build cancelled or superseded.".into());
        }
        let mut store = self
            .store
            .lock()
            .map_err(|_| "Build artifact store is unavailable.")?;
        let mut complete = BTreeMap::<String, Arc<T>>::new();
        let mut coalesced = BTreeMap::<ContentKey, Arc<T>>::new();
        for id in &plan.order {
            if stopped() {
                return Err("Build cancelled or superseded.".into());
            }
            let key = plan.keys[id];
            let cached = store.get(self.retention, &key);
            let value = if let Some(value) = cached.or_else(|| coalesced.get(&key).cloned()) {
                value
            } else {
                let node = &plan.nodes[id];
                let inputs = node
                    .dependencies
                    .iter()
                    .map(|dependency| (dependency.role.clone(), complete[&dependency.node].clone()))
                    .collect();
                let producer_stopped = || {
                    if !stopped() {
                        return false;
                    }
                    let latest = self.latest.load(Ordering::Acquire);
                    self.interests.lock().map_or(true, |registered| {
                        registered.get(&(latest, key)).copied().unwrap_or(0)
                            <= usize::from(latest == revision)
                    })
                };
                let (value, bytes) = produce(node, &inputs, &producer_stopped)?;
                if producer_stopped() {
                    return Err("Build cancelled or superseded.".into());
                }
                let value = Arc::new(value);
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
