//! Drawing scope for procedural fixtures with material-slot repeats supplied by their owner.
use super::{
    mesh::Mesh,
    molding::{Strip, profiled_strip},
    pen::{Pen, Tint},
};
use superspec_geometry::V3;

pub struct Fixture<'a, 'cancel> {
    pub pen: Pen,
    pub mesh: &'a mut Mesh<'cancel>,
    pub repeats: &'a dyn Fn(&str) -> f64,
}
impl Fixture<'_, '_> {
    pub(super) fn repeat(&self, slot: &str) -> f64 {
        if self.mesh.surveying() {
            1.
        } else {
            (self.repeats)(slot)
        }
    }
    pub fn panel(&mut self, slot: &str, span: [f64; 4], y: f64, tint: V3) -> Result<(), String> {
        let repeat = self.repeat(slot);
        self.pen
            .panel(self.mesh, slot, span, y, &Tint::Color(tint), repeat)
    }
    pub fn cuboid(
        &mut self,
        slot: &str,
        low: V3,
        high: V3,
        tint: V3,
        omit: &str,
    ) -> Result<(), String> {
        let repeat = self.repeat(slot);
        self.pen
            .cuboid(self.mesh, slot, low, high, &Tint::Color(tint), repeat, omit)
    }
    pub fn bar(&mut self, slot: &str, a: V3, b: V3, width: f64, tint: V3) -> Result<(), String> {
        let repeat = self.repeat(slot);
        self.pen
            .bar(self.mesh, slot, a, b, width, &Tint::Color(tint), repeat)
    }
    pub fn tube(
        &mut self,
        slot: &str,
        points: &[V3],
        radius: f64,
        tint: V3,
        sides: usize,
    ) -> Result<(), String> {
        let repeat = self.repeat(slot);
        self.pen.tube(
            self.mesh,
            slot,
            points,
            radius,
            sides,
            &Tint::Color(tint),
            repeat,
        )
    }
    pub fn disc(
        &mut self,
        slot: &str,
        center: V3,
        radius: f64,
        normal: V3,
        tint: V3,
        sides: usize,
    ) -> Result<(), String> {
        let repeat = self.repeat(slot);
        self.pen.disc(
            self.mesh,
            slot,
            center,
            radius,
            normal,
            sides,
            &Tint::Color(tint),
            repeat,
        )
    }
    pub fn drum(
        &mut self,
        slot: &str,
        center: V3,
        radius: f64,
        height: f64,
        tint: V3,
        sides: usize,
    ) -> Result<(), String> {
        self.pen.drum(
            self.mesh,
            slot,
            center,
            radius,
            height,
            sides,
            &Tint::Color(tint),
            self.repeat(slot),
        )
    }
    pub fn face(&mut self, slot: &str, points: &[V3], normal: V3, tint: V3) -> Result<(), String> {
        let repeat = self.repeat(slot);
        self.pen.face(
            self.mesh,
            slot,
            points,
            normal,
            &Tint::Color(tint),
            repeat,
            None,
            None,
        )
    }
    pub fn sheet(&mut self, slot: &str, points: &[V3], tint: V3) -> Result<(), String> {
        let repeat = self.repeat(slot);
        self.pen.sheet(
            self.mesh,
            slot,
            points,
            &Tint::Color(tint),
            &Tint::Color(tint),
            repeat,
        )
    }
    pub fn molding(
        &mut self,
        slot: &str,
        span: [f64; 2],
        section: &[[f64; 2]],
        tint: V3,
    ) -> Result<(), String> {
        let repeat = self.repeat(slot);
        profiled_strip(
            &self.pen,
            self.mesh,
            slot,
            &Strip {
                span,
                section,
                omitted_edges: &[],
                miters: [None; 2],
                setback: 0.,
            },
            &Tint::Color(tint),
            repeat,
        )
        .map(|_| ())
    }
    /// A chamfered frame section stays within its source box, including short latch pieces.
    pub fn frame_box(&mut self, low: V3, high: V3, color: V3) -> Result<(), String> {
        let size: V3 = std::array::from_fn(|i| high[i] - low[i]);
        let mut axis = 0;
        for i in 1..3 {
            if size[i] > size[axis] {
                axis = i;
            }
        }
        let across = if axis == 0 { 2 } else { 0 };
        let depth = (0..3).find(|i| *i != axis && *i != across).unwrap();
        let a = std::array::from_fn(|i| {
            if i == axis {
                low[i]
            } else {
                (low[i] + high[i]) / 2.
            }
        });
        let mut b = a;
        b[axis] = high[axis];
        let mut side = [0.; 3];
        side[across] = 1.;
        let repeat = self.repeat("paint");
        self.pen.beveled_bar(
            self.mesh,
            "paint",
            a,
            b,
            side,
            size[across],
            size[depth],
            0.0009,
            &Tint::Color(color),
            repeat,
        )
    }
}
