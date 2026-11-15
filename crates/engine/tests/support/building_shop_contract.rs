//! Integration ownership for native interiors; independent component geometry is
//! checked by the grocer/greengrocer/roast and structural shell suites.
use serde::Deserialize;
use superspec_engine::Scene;
use superspec_geometry::{Matrix, V3};

#[derive(Deserialize)]
pub struct Room {
    host: String,
    trade: String,
    matrix: [f64; 16],
    plan: Plan,
}
#[derive(Deserialize)]
struct Plan {
    x0: f64,
    x1: f64,
    floor: f64,
    head: f64,
    depth: f64,
}
impl Room {
    fn part(&self) -> String {
        format!("{}/$dressing/shop-{}", self.host, self.trade)
    }
    fn inverse(&self) -> Matrix {
        Matrix(std::array::from_fn(|column| {
            std::array::from_fn(|axis| self.matrix[column * 4 + axis])
        }))
        .inverse()
        .unwrap()
    }
    fn local(&self, position: V3) -> V3 {
        let p = self.inverse().point(position);
        [p[0] - self.plan.x0, -p[1], p[2] - self.plan.floor]
    }
    fn contains(&self, p: V3) -> bool {
        let p = self.local(p);
        p.iter()
            .zip([
                self.plan.x1 - self.plan.x0,
                self.plan.depth,
                self.plan.head - self.plan.floor,
            ])
            .all(|(&v, upper)| v >= -0.0001 && v <= upper + 0.0001)
    }
}

pub fn native_part(rooms: &[Room], part: &str) -> bool {
    rooms.iter().any(|room| {
        let root = room.part();
        part == root
            || part.strip_prefix(&root).is_some_and(|suffix| {
                matches!(
                    suffix,
                    "/back-stock/rear-timber"
                        | "/back-stock/rear-steel"
                        | "/checkout"
                        | "/services"
                        | "/room-detail"
                )
            })
    })
}

pub fn check(scene: &Scene, rooms: &[Room]) {
    for room in rooms {
        assert!(
            ["grocer", "greengrocer", "roast"].contains(&room.trade.as_str()),
            "only reviewed native compositions may replace a legacy oracle"
        );
    }
    for mesh in scene
        .manifest
        .meshes
        .iter()
        .filter(|m| native_part(rooms, &m.object_id))
    {
        assert!(mesh.visible && mesh.index_count > 0);
        let material = mesh.material.as_ref().expect("native interior material");
        assert!(scene.manifest.materials.iter().any(|m| &m.id == material));
        let candidates: Vec<_> = rooms
            .iter()
            .filter(|r| mesh.object_id == r.part() || mesh.object_id.starts_with(&(r.part() + "/")))
            .collect();
        for index in &scene.indices[mesh.index_start..mesh.index_start + mesh.index_count] {
            let index = mesh.vertex_start + *index as usize;
            let p = std::array::from_fn(|axis| scene.positions[index * 3 + axis] as f64);
            assert!(
                candidates.iter().any(|r| r.contains(p)),
                "{} escapes independently planned room: {p:?}",
                mesh.object_id
            );
            assert!(
                scene.normals[index * 3..index * 3 + 3]
                    .iter()
                    .chain(&scene.colors[index * 3..index * 3 + 3])
                    .chain(&scene.uvs[index * 2..index * 2 + 2])
                    .all(|v| v.is_finite())
            );
        }
    }
    for room in rooms {
        let part = room.part();
        let points = |suffix: &str| {
            let id = part.clone() + suffix;
            let result: Vec<_> = scene
                .manifest
                .meshes
                .iter()
                .filter(|m| m.object_id == id)
                .flat_map(|m| {
                    scene.indices[m.index_start..m.index_start + m.index_count]
                        .iter()
                        .filter_map(|&i| {
                            let i = m.vertex_start + i as usize;
                            let p =
                                std::array::from_fn(|axis| scene.positions[i * 3 + axis] as f64);
                            room.contains(p).then(|| room.local(p))
                        })
                })
                .collect();
            assert!(!result.is_empty(), "missing independently planned {id}");
            result
        };
        let body = points("");
        assert!(
            body.iter().any(|p| p[2].abs() < 0.0001),
            "interior fittings lack floor support"
        );
        assert!(
            body.iter().any(|p| p[2] > 0.6),
            "interior stock lacks vertical extent"
        );
        if room.trade == "greengrocer" {
            for suffix in [
                "/back-stock/rear-timber",
                "/back-stock/rear-steel",
                "/checkout",
            ] {
                let p = points(suffix);
                assert!(
                    p.iter().any(|p| p[2].abs() < 0.0001),
                    "{part}{suffix} lacks floor support"
                );
            }
        }
        let width = room.plan.x1 - room.plan.x0;
        let height = room.plan.head - room.plan.floor;
        if height >= 2.3 {
            let services = points("/services");
            assert!(services.iter().all(|p| p[2] >= height - 0.13));
            for edge in [0.008, width - 0.008] {
                assert!(
                    services.iter().any(|p| (p[0] - edge).abs() < 0.0001),
                    "service boxes must reach both side walls"
                );
            }
        }
        if ["grocer", "greengrocer", "roast"].contains(&room.trade.as_str()) {
            let details = points("/room-detail");
            for edge in [0.026, width - 0.026] {
                assert!(
                    details.iter().any(|p| (p[0] - edge).abs() < 0.0001),
                    "room wear must cover both side walls"
                );
            }
            let floor = format!("{}/$dressing/envelope/room-floor", room.host);
            let meshes: Vec<_> = scene
                .manifest
                .meshes
                .iter()
                .filter(|m| m.object_id == floor)
                .collect();
            assert!(!meshes.is_empty(), "missing tiled room floor");
            assert!(
                meshes.iter().all(|m| m.visible
                    && m.material
                        .as_ref()
                        .is_some_and(|id| id.starts_with("$surface/raster/"))),
                "floor must bind its tile receiver, not the unbaked wall material"
            );
        }
    }
}
