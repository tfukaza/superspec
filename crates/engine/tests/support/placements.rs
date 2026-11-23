//! What a placement promises, checked against the expanded meshes it annotates: placing its
//! prototype by `ItemGeometry::placed` gives those meshes, corner for corner.
use superspec_engine::itemspec::definition::ItemGeometry;
use superspec_geometry::Matrix;

/// How closely a rebuilt corner matches: positions and unit normals in metres, colours in
/// their own unit.
pub struct Tolerance {
    pub position: f64,
    pub colour: f64,
}

/// Rebuilds every placed mesh of `geometry` from its prototype and compares it with the
/// flattened mesh it covers. Returns the triangles the placements cover.
pub fn rebuilds_the_expanded_meshes(geometry: &ItemGeometry, tolerance: &Tolerance) -> usize {
    let flattened = geometry.flattened().unwrap();
    // Flattening numbers meshes node by node.
    let mut flat = vec![usize::MAX; geometry.meshes.len()];
    for (at, &mesh) in geometry
        .nodes
        .iter()
        .flat_map(|node| &node.meshes)
        .enumerate()
    {
        flat[mesh] = at;
    }
    let mut covered = vec![false; geometry.meshes.len()];
    let mut triangles = 0;
    for placement in geometry.placed().unwrap() {
        let path = &placement.path;
        let prototype = geometry
            .prototypes
            .get(&placement.prototype)
            .unwrap_or_else(|| panic!("{path} names a prototype the item does not carry"));
        assert_eq!(
            prototype.own_key().unwrap(),
            placement.prototype,
            "{path}: the prototype is filed under its own key"
        );
        assert_eq!(
            prototype.roles.iter().collect::<Vec<_>>(),
            placement.inputs.keys().collect::<Vec<_>>(),
            "{path}: one input per role"
        );
        let matrix = Matrix(placement.transform);
        assert!(
            matrix.determinant() > 0.,
            "{path}: a placement is a proper motion"
        );
        let inverse = matrix.inverse().unwrap();
        let sources: Vec<_> = prototype
            .meshes
            .iter()
            .filter(|mesh| placement.slots.contains_key(&mesh.material_slot))
            .collect();
        assert_eq!(sources.len(), placement.meshes.len(), "{path}");
        for (source, &index) in sources.iter().zip(&placement.meshes) {
            assert!(
                !std::mem::replace(&mut covered[index], true),
                "{path}: mesh {index} is covered twice"
            );
            let expanded = &flattened[flat[index]];
            assert_eq!(
                expanded.material_slot, placement.slots[&source.material_slot],
                "{path}"
            );
            assert!(
                expanded.id.contains(path.rsplit('/').next().unwrap()),
                "{path} covers {}",
                expanded.id
            );
            assert_eq!(source.indices.len(), expanded.indices.len(), "{path}");
            assert_eq!(source.colors.is_empty(), expanded.colors.is_empty());
            assert_eq!(source.uvs.is_empty(), expanded.uvs.is_empty());
            triangles += source.indices.len() / 3;
            for (&from, &to) in source.indices.iter().zip(&expanded.indices) {
                let (from, to) = (from as usize, to as usize);
                let position = matrix.point(source.positions[from]);
                let normal = source.normals[from];
                let normal: [f64; 3] = std::array::from_fn(|axis| {
                    (0..3).map(|k| inverse.0[axis][k] * normal[k]).sum::<f64>()
                });
                let length = normal.iter().map(|v| v * v).sum::<f64>().sqrt();
                for axis in 0..3 {
                    assert!(
                        (position[axis] - expanded.positions[to][axis]).abs() <= tolerance.position,
                        "{path}: {position:?} is not {:?}",
                        expanded.positions[to]
                    );
                    assert!(
                        (normal[axis] / length - expanded.normals[to][axis]).abs()
                            <= tolerance.position.max(1e-9),
                        "{path}: normal {normal:?} is not {:?}",
                        expanded.normals[to]
                    );
                }
                if let (Some(a), Some(b)) = (source.uvs.get(from), expanded.uvs.get(to)) {
                    assert_eq!(a, b, "{path}: texture coordinates");
                }
                if let (Some(shade), Some(colour)) =
                    (source.colors.get(from), expanded.colors.get(to))
                {
                    let tint = match source.roles.get(from).copied().unwrap_or(0) {
                        0 => [1.; 3],
                        role => placement.inputs[&prototype.roles[usize::from(role) - 1]],
                    };
                    for channel in 0..4 {
                        let rebuilt = shade[channel] * tint.get(channel).copied().unwrap_or(1.);
                        assert!(
                            (rebuilt - colour[channel]).abs() <= tolerance.colour,
                            "{path}: colour {rebuilt} is not {} (role {:?})",
                            colour[channel],
                            source.roles.get(from)
                        );
                    }
                }
            }
        }
    }
    triangles
}
