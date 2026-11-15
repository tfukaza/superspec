use crate::{Profile, math::*};
use manifold_rust::{
    linalg::{Mat3x4, Vec3},
    manifold::Manifold,
    types::{Error, MeshGL, OpType},
};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default)]
pub struct Geometry {
    pub points: Vec<V3>,
    pub triangles: Vec<[usize; 3]>,
}
#[derive(Clone, Copy)]
pub enum Operation {
    Union,
    Difference,
    Intersection,
}
#[derive(Clone)]
pub struct Solid(Manifold);
fn vector(p: V3) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}
impl Solid {
    pub fn bounds(&self) -> [V3; 2] {
        let bounds = self.0.bounding_box();
        [
            [bounds.min.x, bounds.min.y, bounds.min.z],
            [bounds.max.x, bounds.max.y, bounds.max.z],
        ]
    }
    pub fn trim(&self, normal: V3, offset: f64) -> Result<Self, String> {
        Self::checked(self.0.trim_by_plane(vector(normal), offset))
    }
    pub fn split(&self, normal: V3, offset: f64) -> Result<[Self; 2], String> {
        let (a, b) = self.0.split_by_plane(vector(normal), offset);
        Ok([Self::checked(a)?, Self::checked(b)?])
    }
    pub fn components(&self) -> Result<Vec<Self>, String> {
        self.0.decompose().into_iter().map(Self::checked).collect()
    }
    /// Projects a closed solid onto XY, preserving holes in its silhouette.
    pub fn project_xy(&self) -> Vec<Vec<V3>> {
        self.0
            .project()
            .to_polygons()
            .iter()
            .map(|ring| ring.iter().map(|p| [p.x, p.y, 0.]).collect())
            .collect()
    }
    pub fn slice_xy(&self, height: f64) -> Vec<Vec<V3>> {
        self.0
            .slice(height)
            .to_polygons()
            .iter()
            .map(|ring| ring.iter().map(|p| [p.x, p.y, height]).collect())
            .collect()
    }
    /// Produces a triangulated XY surface after a planar polygon difference.
    pub fn planar_surface(footprints: &[Vec<V3>], remove: &[Vec<V3>]) -> Result<Geometry, String> {
        use manifold_rust::{cross_section::CrossSection, linalg::Vec2};
        let section = |polygons: &[Vec<V3>]| {
            CrossSection::new(
                polygons
                    .iter()
                    .map(|ring| ring.iter().map(|p| Vec2::new(p[0], p[1])).collect())
                    .collect(),
            )
        };
        let loops: Vec<Vec<V3>> = section(footprints)
            .difference(&section(remove))
            .to_polygons()
            .iter()
            .map(|ring| ring.iter().map(|p| [p.x, p.y, 0.]).collect())
            .collect();
        if loops.is_empty() {
            return Ok(Geometry::default());
        }
        let solid = Self::extrude(&loops, 1.)?;
        let mesh = solid.mesh();
        let points: Vec<V3> = mesh
            .vert_properties
            .chunks_exact(mesh.num_prop as usize)
            .map(|p| [p[0] as f64, p[1] as f64, p[2] as f64])
            .collect();
        let triangles = mesh
            .tri_verts
            .chunks_exact(3)
            .map(|t| [t[0] as usize, t[1] as usize, t[2] as usize])
            .filter(|t| t.iter().all(|i| points[*i][2] > 0.5))
            .collect();
        Ok(Geometry {
            points: points.into_iter().map(|p| [p[0], p[1], 0.]).collect(),
            triangles,
        })
    }
    pub fn warp(&self, function: impl Fn(V3) -> V3) -> Result<Self, String> {
        Self::checked(self.0.warp(|point| {
            *point = vector(function([point.x, point.y, point.z]));
        }))
    }
    pub fn extrude(loops: &[Vec<V3>], height: f64) -> Result<Self, String> {
        let polygons = loops
            .iter()
            .map(|ring| {
                ring.iter()
                    .map(|p| manifold_rust::linalg::Vec2::new(p[0], p[1]))
                    .collect()
            })
            .collect();
        Self::checked(Manifold::extrude(
            &polygons,
            height,
            0,
            0.,
            manifold_rust::linalg::Vec2::new(1., 1.),
        ))
    }
    pub fn inset(ring: &[V3], distance: f64) -> Vec<Vec<V3>> {
        let points = ring
            .iter()
            .map(|p| manifold_rust::linalg::Vec2::new(p[0], p[1]))
            .collect();
        manifold_rust::cross_section::CrossSection::new(vec![points])
            .offset_with_params(-distance, 2, 2., 0)
            .to_polygons()
            .iter()
            .map(|ring| ring.iter().map(|p| [p.x, p.y, 0.]).collect())
            .collect()
    }
    pub fn uncovered_area(footprint: &[Vec<V3>], support: &[Vec<V3>]) -> f64 {
        use manifold_rust::{cross_section::CrossSection, linalg::Vec2};
        let section = |polygons: &[Vec<V3>]| {
            CrossSection::new(
                polygons
                    .iter()
                    .map(|p| p.iter().map(|p| Vec2::new(p[0], p[1])).collect())
                    .collect(),
            )
        };
        section(footprint).difference(&section(support)).area()
    }
    pub fn checked(value: Manifold) -> Result<Self, String> {
        if value.status() != Error::NoError {
            Err(format!("Invalid closed solid: {}.", value.status()))
        } else {
            Ok(Self(value))
        }
    }
    pub fn cube(size: V3) -> Result<Self, String> {
        Self::checked(Manifold::cube(vector(size), false))
    }
    pub fn cylinder(height: f64, radius: f64, top: f64, segments: usize) -> Result<Self, String> {
        Self::checked(Manifold::cylinder(height, radius, top, segments as i32))
    }
    pub fn sphere(radius: f64, segments: usize) -> Result<Self, String> {
        if segments
            .div_ceil(4)
            .checked_pow(2)
            .and_then(|n| n.checked_mul(8))
            .is_none_or(|n| n > 1_000_000)
        {
            return Err("Sphere triangle budget exceeded before allocation.".into());
        }
        Self::checked(Manifold::sphere(radius, segments as i32))
    }
    pub fn triangle_count(&self) -> usize {
        self.0.num_tri()
    }
    pub fn transform(&self, m: Matrix) -> Result<Self, String> {
        Self::checked(self.0.transform(&Mat3x4::from_cols(
            vector(m.0[0]),
            vector(m.0[1]),
            vector(m.0[2]),
            vector(m.0[3]),
        )))
    }
    pub fn boolean(&self, other: &Self, op: Operation) -> Result<Self, String> {
        Self::checked(self.0.boolean(
            &other.0,
            match op {
                Operation::Union => OpType::Add,
                Operation::Difference => OpType::Subtract,
                Operation::Intersection => OpType::Intersect,
            },
        ))
    }
    pub fn union(parts: &[Self]) -> Result<Self, String> {
        if parts.is_empty() {
            return Err("A physical collection has no closed operands.".into());
        }
        Self::checked(Manifold::batch_boolean(
            &parts.iter().map(|s| s.0.clone()).collect::<Vec<_>>(),
            OpType::Add,
        ))
    }
    pub fn volume(&self) -> f64 {
        self.0.volume()
    }
    pub fn area(&self) -> f64 {
        self.0.surface_area()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn mesh(&self) -> MeshGL {
        self.0.get_mesh_gl(-1)
    }
    /// Boundary loops of the solid projected into its local XZ plane.
    pub fn silhouette_xz(&self) -> Vec<Vec<V3>> {
        self.0
            .rotate(-90., 0., 0.)
            .project()
            .to_polygons()
            .iter()
            .map(|ring| ring.iter().map(|p| [p.x, p.y, 0.]).collect())
            .collect()
    }
    /// Reimports at the existing f32 physical-kernel boundary with explicit source ownership.
    pub fn owned(&self, owner: u32) -> Result<Self, String> {
        let mut mesh = self.mesh();
        mesh.run_index = vec![0, mesh.tri_verts.len() as u32];
        mesh.run_original_id = vec![owner];
        mesh.run_transform.clear();
        Self::checked(Manifold::from_mesh_gl(&mesh))
    }
    pub fn reserve_owner() -> u32 {
        Manifold::reserve_ids(1)
    }
    pub fn from_geometry(geometry: &Geometry, owner: u32) -> Result<Self, String> {
        let mut points = vec![];
        let mut remap = vec![];
        let mut lookup = BTreeMap::new();
        for p in &geometry.points {
            let p = p.map(|v| v as f32);
            if p.iter().any(|v| !v.is_finite()) {
                return Err("Nonfinite mesh coordinate.".into());
            }
            let key = p.map(|v| if v == 0. { 0 } else { v.to_bits() });
            let index = *lookup.entry(key).or_insert_with(|| {
                let i = points.len() / 3;
                points.extend(p);
                i
            });
            remap.push(index as u32);
        }
        let mut triangles = vec![];
        for t in &geometry.triangles {
            if t.iter().any(|i| *i >= remap.len()) {
                return Err("Mesh index out of bounds.".into());
            }
            let ids = t.map(|i| remap[i]);
            if ids[0] == ids[1] || ids[1] == ids[2] || ids[0] == ids[2] {
                return Err("Mesh collapses at f32 storage precision.".into());
            }
            triangles.extend(ids);
        }
        let mesh = MeshGL {
            num_prop: 3,
            vert_properties: points,
            run_index: vec![0, triangles.len() as u32],
            run_original_id: vec![owner],
            tri_verts: triangles,
            ..MeshGL::default()
        };
        Self::checked(Manifold::from_mesh_gl(&mesh))
    }
}
impl Geometry {
    pub fn transformed(&self, matrix: Matrix) -> Self {
        Self {
            points: self.points.iter().map(|p| matrix.point(*p)).collect(),
            triangles: self
                .triangles
                .iter()
                .map(|t| {
                    if matrix.determinant() < 0. {
                        [t[2], t[1], t[0]]
                    } else {
                        *t
                    }
                })
                .collect(),
        }
    }
    /// Closes a triangulated top to corresponding bottom positions, retaining holes.
    pub fn surface(profile: &Profile, bottom: impl Fn(V3) -> V3) -> Self {
        let count = profile.points.len();
        let mut points = profile.points.clone();
        points.extend(profile.points.iter().map(|p| bottom(*p)));
        let mut triangles = profile.triangles.clone();
        triangles.extend(
            profile
                .triangles
                .iter()
                .map(|[a, b, c]| [c + count, b + count, a + count]),
        );
        let mut boundary = BTreeMap::new();
        for t in &profile.triangles {
            for i in 0..3 {
                let (a, b) = (t[i], t[(i + 1) % 3]);
                let key = (a.min(b), a.max(b));
                if boundary.remove(&key).is_none() {
                    boundary.insert(key, (a, b));
                }
            }
        }
        for (a, b) in boundary.into_values() {
            triangles.extend([[a, a + count, b + count], [a, b + count, b]]);
        }
        Self { points, triangles }
    }
    pub fn loft(sections: &[Vec<V3>], caps: &[[usize; 3]]) -> Result<Self, String> {
        if sections.len() < 2
            || sections[0].is_empty()
            || sections.iter().any(|s| s.len() != sections[0].len())
        {
            return Err("Loft requires matching nonempty sections.".into());
        }
        let n = sections[0].len();
        if sections
            .len()
            .checked_mul(n)
            .is_none_or(|count| count > 500_000)
        {
            return Err("Loft vertex budget exceeded before allocation.".into());
        }
        let points = sections.concat();
        let mut triangles = vec![];
        let mut boundary = BTreeMap::new();
        for &[a, b, c] in caps {
            triangles.extend([
                [c, b, a],
                [
                    a + points.len() - n,
                    b + points.len() - n,
                    c + points.len() - n,
                ],
            ]);
            let t = [a, b, c];
            for i in 0..3 {
                let (a, b) = (t[i], t[(i + 1) % 3]);
                let key = (a.min(b), a.max(b));
                if boundary.remove(&key).is_none() {
                    boundary.insert(key, (a, b));
                }
            }
        }
        for s in 1..sections.len() {
            for &(i, j) in boundary.values() {
                let (a, b, c, d) = ((s - 1) * n + i, (s - 1) * n + j, s * n + j, s * n + i);
                triangles.extend([[a, b, c], [a, c, d]]);
            }
        }
        Ok(Self { points, triangles })
    }
}
