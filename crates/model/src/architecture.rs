//! Architectural provenance and physical-context delivery records.
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use superspec_geometry::Matrix;
use superspec_types::{
    architecture::{Node, V3, Value},
    diagnostic::SourceLocation,
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Object {
    pub id: String,
    pub source_id: String,
    pub kind: String,
    pub source: SourceLocation,
    pub matrix: [V3; 4],
    pub instances: Vec<String>,
    pub assemblies: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub contributors: Vec<String>,
    pub attributes: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Floor {
    pub id: String,
    pub area: String,
    pub points: Vec<V3>,
    pub triangles: Vec<[usize; 3]>,
    pub loops: Vec<Vec<V3>>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Connection {
    pub id: String,
    pub from: String,
    pub to: String,
    pub path: Vec<V3>,
    pub source: SourceLocation,
}
#[derive(Clone, Debug, Serialize)]
pub struct MouthObstruction {
    pub height: f64,
    pub blockers: Vec<String>,
}
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Volume {
    pub id: String,
    pub kind: String,
    pub role: Option<String>,
    pub movement: String,
    pub points: Vec<V3>,
    pub triangles: Vec<[usize; 3]>,
    pub loops: Vec<Vec<V3>>,
    pub extrusion: V3,
    pub require_support: bool,
    pub properties: BTreeMap<String, String>,
    pub source: SourceLocation,
    pub bounds: [V3; 2],
}
#[derive(Clone, Debug, Serialize)]
pub struct Context {
    pub metadata: Vec<Node>,
    pub declarations: Vec<Node>,
    pub objects: Vec<Object>,
    pub floors: Vec<Floor>,
    pub water: Vec<Floor>,
    pub water_obstacles: BTreeMap<String, Vec<Vec<V3>>>,
    pub connections: Vec<Connection>,
    pub connection_obstructions: BTreeMap<String, MouthObstruction>,
    pub volumes: Vec<Volume>,
}
impl Context {
    /// Records cross-host visual ancestry after verifying every generated object and contributor.
    pub fn credit_visuals(
        &mut self,
        credits: &BTreeMap<String, Vec<String>>,
    ) -> Result<(), String> {
        let mut bytes = 0usize;
        for (part, hosts) in credits {
            bytes = bytes
                .checked_add(part.len() + hosts.iter().map(String::len).sum::<usize>())
                .ok_or("Visual ancestry budget overflow.")?;
            if bytes > 4 * 1024 * 1024
                || hosts.is_empty()
                || hosts.len() > 512
                || hosts.iter().collect::<BTreeSet<_>>().len() != hosts.len()
            {
                return Err("Visual contributors exceed ancestry allowance.".into());
            }
            let object = self
                .objects
                .iter()
                .find(|o| &o.id == part)
                .ok_or("Unknown contributed visual object.")?;
            if !matches!(object.attributes.get("host"), Some(Value::Text(host)) if host == &hosts[0])
                || hosts
                    .iter()
                    .any(|host| !self.objects.iter().any(|o| &o.id == host))
            {
                return Err("Visual contributors must start with their owning host and reference existing objects.".into());
            }
        }
        for object in &mut self.objects {
            if let Some(hosts) = credits.get(&object.id) {
                object.contributors = hosts.clone();
            }
        }
        Ok(())
    }
    /// Prepares generated visual ownership without modifying the published context.
    pub fn visual_objects(
        &self,
        host: &str,
        parts: &BTreeSet<String>,
        maximum_bytes: usize,
    ) -> Result<(Vec<Object>, usize), String> {
        let source = self
            .objects
            .iter()
            .find(|n| n.id == host)
            .ok_or("Unknown procedural host.")?;
        let inherited_bytes = source
            .instances
            .iter()
            .chain(&source.assemblies)
            .map(String::len)
            .sum::<usize>()
            + source.source_id.len()
            + host.len() * 2
            + 512;
        let ancestry_bytes = parts
            .iter()
            .try_fold(0usize, |total, part| {
                total.checked_add(inherited_bytes + part.len())
            })
            .ok_or("Generated source ancestry byte budget overflow.")?;
        if ancestry_bytes > maximum_bytes {
            return Err("Generated source ancestry byte budget exceeded.".into());
        }
        let objects = parts
            .iter()
            .map(|part| {
                let id = format!("{host}/$dressing/{part}");
                if self.objects.iter().any(|n| n.id == id) {
                    return Err(format!("Generated object identity is occupied: {id}"));
                }
                Ok(Object {
                    id,
                    source_id: source.source_id.clone(),
                    kind: "visual-mesh".into(),
                    source: source.source.clone(),
                    matrix: Matrix::default().0,
                    instances: source.instances.clone(),
                    assemblies: source.assemblies.clone(),
                    contributors: vec![],
                    attributes: BTreeMap::from([
                        ("host".into(), Value::Text(host.into())),
                        ("role".into(), Value::Text("visual-mesh".into())),
                        ("visible".into(), Value::Boolean(true)),
                        ("collidable".into(), Value::Boolean(false)),
                        ("dynamic".into(), Value::Boolean(false)),
                        ("group".into(), Value::Text("props".into())),
                    ]),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok((objects, ancestry_bytes))
    }
    pub fn source(&self, host: &str) -> Option<&SourceLocation> {
        self.objects
            .iter()
            .find(|n| n.id == host)
            .map(|n| &n.source)
    }
}
