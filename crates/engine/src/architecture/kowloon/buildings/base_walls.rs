//! A procedural envelope may replace only the render skin of its complete native frontage wall.
use super::*;
impl Compiler<'_> {
    pub(super) fn base_walls(
        &self,
        recipes: &[Recipe],
        render: &BTreeMap<u32, Geometry>,
    ) -> Result<BTreeSet<usize>, Diagnostic> {
        if recipes.iter().all(|r| r.request.base_wall.is_none()) {
            return Ok(BTreeSet::new());
        }
        let mut claimed = BTreeSet::new();
        let mut replaced = BTreeSet::new();
        let mut by_node = BTreeMap::<usize, Vec<(&Surface, &Geometry)>>::new();
        for (owner, geometry) in render {
            let surface = &self.surfaces[owner];
            by_node
                .entry(surface.node)
                .or_default()
                .push((surface, geometry));
        }
        let mut inspected = 0usize;
        for recipe in recipes {
            let Some(id) = &recipe.request.base_wall else {
                continue;
            };
            let fail = |message: &str| {
                let mut d = recipe.rule.fail("AML_DRESSING", message);
                d.objects = vec![recipe.request.host.clone()];
                d
            };
            let index = *self
                .resolution
                .ids
                .get(id)
                .ok_or_else(|| fail("base-wall must name a native collidable wall."))?;
            let wall = &self.resolution.nodes[index];
            if wall.source.kind != "wall" || !wall.source.boolean("collidable", true) {
                return Err(fail("base-wall must name a native collidable wall."));
            }
            if !claimed.insert(index) {
                return Err(fail("A base wall can belong to only one building."));
            }
            if self
                .dressing_settings()
                .unwrap()
                .children
                .iter()
                .any(|r| r.text("host") == Some(id.as_str()))
            {
                return Err(fail("Dress the building that owns this base wall."));
            }
            let building::Shape::Box {
                width,
                depth,
                height,
                bottom_center,
            } = recipe.request.shape
            else {
                return Err(fail("Polygon buildings cannot own base walls."));
            };
            let inverse = recipe.request.matrix.inverse().at(&recipe.rule)?;
            let offset = if bottom_center {
                [-width / 2., -depth / 2., 0.]
            } else {
                [0.; 3]
            };
            let bounds = [width, depth, height - ROOF_RESERVE];
            let axis = self
                .path(&wall.source, "path")?
                .iter()
                .map(|p| inverse.point(wall.matrix.point(*p)))
                .collect::<Vec<_>>();
            if axis
                .iter()
                .any(|p| (p[1] - axis[0][1]).abs() > 1e-5 || (p[2] - axis[0][2]).abs() > 1e-5)
            {
                return Err(fail(
                    "The base wall must run parallel to the building frontage at one elevation.",
                ));
            }
            let mut left = f64::INFINITY;
            let mut right = f64::NEG_INFINITY;
            let mut frontage = false;
            let mut ground = false;
            let mut found = false;
            for (surface, geometry) in by_node.get(&index).into_iter().flatten() {
                if surface.assigned {
                    return Err(fail(
                        "A building base wall cannot replace explicitly assigned faces.",
                    ));
                }
                inspected += geometry.triangles.len();
                if inspected > 1_048_576 {
                    return Err(fail(
                        "Base-wall validation exceeds scene triangle allowance.",
                    ));
                }
                for triangle in &geometry.triangles {
                    found = true;
                    for &vertex in triangle {
                        let point = inverse.point(geometry.points[vertex]);
                        left = left.min(point[0]);
                        right = right.max(point[0]);
                        frontage |= (point[1] - offset[1]).abs() < 1e-5;
                        ground |= point[2].abs() < 1e-5;
                        if (0..3).any(|a| {
                            point[a] - offset[a] < -1e-5 || point[a] - offset[a] > bounds[a] + 1e-5
                        }) {
                            return Err(fail(
                                "The building envelope must contain its complete base wall.",
                            ));
                        }
                    }
                }
            }
            if !found {
                return Err(fail("The base wall has no exposed native geometry."));
            }
            if (left - offset[0]).abs() > 1e-5 || (right - offset[0] - width).abs() > 1e-5 {
                return Err(fail(
                    "The base wall must span the complete building frontage.",
                ));
            }
            if !frontage || !ground {
                return Err(fail(
                    "The base wall must meet the building front and ground planes.",
                ));
            }
            for (opening, node) in self
                .resolution
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| n.source.kind == "opening")
            {
                if self
                    .resolution
                    .reference(opening, node.source.text("host").unwrap())?
                    == index
                {
                    return Err(fail(
                        "A building base wall must be solid; functional entrances require authored building interiors.",
                    ));
                }
            }
            if wall.source.boolean("dynamic", false) {
                return Err(fail("A building base wall must be static."));
            }
            if self.resolution.nodes[recipe.node]
                .source
                .boolean("visible", true)
            {
                replaced.insert(index);
            }
        }
        Ok(replaced)
    }
}
