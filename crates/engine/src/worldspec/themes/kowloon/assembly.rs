//! Per-build Kowloon planning state and explicitly supplied execution services.
use super::{cache::Cache, floor::Finish, pavement::plan::Settings};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
};
use superspec_types::architecture::{Node, V3};

pub(crate) struct FloorPlan {
    pub(crate) source: Node,
    pub(crate) material: String,
    pub(crate) finish: Finish,
    pub(crate) ground: Option<Settings>,
}

#[derive(Default)]
pub(crate) struct ThemeState {
    pub(crate) floor_dressing: BTreeMap<usize, FloorPlan>,
    pub(crate) building_hosts: BTreeSet<usize>,
    pub(crate) render_edge_vertices: Vec<V3>,
    pub(crate) sign_survey: Vec<[V3; 3]>,
}

pub(crate) struct Execution<'a> {
    pub(crate) cache: Option<&'a Mutex<Cache>>,
    pub(crate) workers: usize,
    pub(crate) observe: &'a dyn Fn(&str),
}
