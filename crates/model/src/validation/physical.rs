use serde::Serialize;
use superspec_types::architecture::V3;

#[derive(Clone, Serialize, Debug)]
pub struct Component {
    pub area: String,
    pub bounds: [V3; 2],
}
#[derive(Clone, Serialize, Debug)]
pub struct Resource {
    pub kind: &'static str,
    pub declarations: Vec<String>,
    pub from: Vec<usize>,
    pub to: Vec<usize>,
    pub bidirectional: bool,
    pub path: Vec<V3>,
}
#[derive(Clone, Serialize, Debug)]
pub struct Connection {
    pub id: String,
    pub status: &'static str,
    pub path: Vec<V3>,
}
#[derive(Clone, Serialize, Debug, Default)]
pub struct Evidence {
    pub components: Vec<Component>,
    pub resources: Vec<Resource>,
    pub connections: Vec<Connection>,
}
