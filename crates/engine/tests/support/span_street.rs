//! Two facing street walls with a regular utility grid and three windows each, authored inline.
use superspec_engine::worldspec::themes::kowloon::spans::{
    anchors::{Anchor, Plane, Rect},
    plan::{self, Plan, Rule},
    walking::{Surface, Walking},
};
use superspec_geometry::obstructions::{Budget, Obstructions, Triangle};
pub struct Street {
    pub planes: Vec<Plane>,
    pub triangles: Vec<Triangle>,
    pub floors: Walking,
    pub utility: Vec<Anchor>,
    pub windows: Vec<Anchor>,
}
pub const RULE: Rule = Rule {
    cables: 0.8,
    clotheslines: 0.8,
    signs: 1.,
    reach: 14.,
    clearance: 2.6,
};
/// 12 m by 9 m walls `gap` meters apart, standing on a shared collidable floor.
pub fn street(gap: f64, budget: &mut Budget<'_>) -> Street {
    let wall = |y: f64| -> [Triangle; 2] {
        [
            [[0., y, 0.], [12., y, 0.], [12., y, 9.]],
            [[0., y, 0.], [12., y, 9.], [0., y, 9.]],
        ]
    };
    let planes = vec![
        Plane::new("wall-0", [0., 1., 0.], 0., &wall(0.), budget).unwrap(),
        Plane::new("wall-1", [0., -1., 0.], -gap, &wall(gap), budget).unwrap(),
    ];
    // Plane 1 faces -y, so its u axis runs along -x.
    let anchor = |plane: usize, x: f64, z: f64, window: Option<Rect>| Anchor {
        plane,
        u: if plane == 0 { x } else { -x },
        z,
        position: [x, plane as f64 * gap, z],
        height: z,
        window,
    };
    let mut utility = vec![];
    for plane in 0..2 {
        for column in 0..8 {
            for z in [3., 4.1, 5.2, 6.3, 7.4, 8.64] {
                utility.push(anchor(plane, 0.8 + column as f64 * 1.5, z, None));
            }
        }
    }
    let mut windows = vec![];
    for (plane, x0) in [(0, 2.), (0, 5.), (0, 8.), (1, 8.), (1, 5.), (1, 2.)] {
        let (u0, u1) = if plane == 0 {
            (x0, x0 + 1.)
        } else {
            (-x0 - 1., -x0)
        };
        let window = Rect {
            u0,
            u1,
            z0: 3.1,
            z1: 4.6,
        };
        windows.push(anchor(plane, x0 + 1.16, 4.2, Some(window)));
    }
    let floors = Walking::new(
        &[
            [[-10., -10., 0.], [25., -10., 0.], [25., 25., 0.]],
            [[-10., -10., 0.], [25., 25., 0.], [-10., 25., 0.]],
        ]
        .map(|points| Surface {
            points,
            floor_or_stairs: true,
            collidable: true,
        }),
        budget,
    )
    .unwrap();
    Street {
        planes,
        triangles: wall(0.).into_iter().chain(wall(gap)).collect(),
        floors,
        utility,
        windows,
    }
}
impl Street {
    #[allow(dead_code)]
    pub fn plan(&self, rule: Rule, seed: &str, budget: &mut Budget<'_>) -> Result<Plan, String> {
        let obstacles = Obstructions::new(&self.triangles, budget)?;
        plan::build(
            rule,
            seed,
            &self.planes,
            &self.utility,
            &self.windows,
            &self.floors,
            obstacles,
            budget,
        )
    }
    /// Utility anchors followed by window anchors, the order drawing indexes them in.
    #[allow(dead_code)]
    pub fn anchors(&self) -> Vec<Anchor> {
        self.utility.iter().chain(&self.windows).cloned().collect()
    }
}
