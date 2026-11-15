//! Cross-host sign collection and source-ordered, inspectable placement decisions.
use super::{
    copy::Business,
    mounts::{Mount, Proposal},
    panel::{Settings, Sides},
    survey::{self, Face, Sign, Solid, Verdict},
};
use superspec_geometry::obstructions::{Bounds, Budget, Obstructions};
use superspec_geometry::{
    V3,
    math::{atan2, cross, hypot, sub},
};
pub fn surveyed(proposal: &Proposal, order: usize) -> Result<Sign, String> {
    if proposal.panels.is_empty()
        || proposal.panels.len() > 256
        || !proposal.above.is_finite()
        || proposal.above.abs() > 10000.
        || order > 4096
    {
        return Err("Sign proposal exceeds its panel or elevation allowance.".into());
    }
    let mut solids = vec![];
    let mut faces = vec![];
    let mut area: f64 = 0.;
    for p in &proposal.panels {
        p.design.validate()?;
        let d = &p.design;
        let origin = p.pen.world([0.; 3]);
        let axis = |v| sub(p.pen.world(v), origin);
        let across = axis([1., 0., 0.]);
        let out = axis([0., 1., 0.]);
        let up = axis([0., 0., 1.]);
        let back = -d.depth;
        let front = 0f64.max(0.02 - d.depth);
        let away = 40f64.min(6f64.max(5. * d.width.max(d.height)));
        let look_down =
            (35. * std::f64::consts::PI / 180.).min(atan2(proposal.above.max(0.), away));
        let face = |y, normal| Face {
            center: p.pen.world([d.width / 2., y, d.height / 2.]),
            right: cross(up, normal),
            up,
            normal,
            width: d.width,
            height: d.height,
            look_down,
        };
        faces.push(face(0., out));
        if p.sides == Sides::Both {
            faces.push(face(-d.depth, out.map(|v| -v)))
        }
        solids.push(Solid {
            center: p
                .pen
                .world([d.width / 2., (back + front) / 2., d.height / 2.]),
            axes: [across, out, up],
            half: [d.width / 2., (front - back) / 2., d.height / 2.],
        });
        area = area.max(d.width * d.height);
    }
    let weight = match proposal.mount {
        Mount::Fascia => 3.,
        Mount::Roof => 2.,
        Mount::Blade => 1.5,
        Mount::Hung => 1.2,
        Mount::Wall => 1.,
        Mount::Balcony => 0.8,
    };
    let sign = Sign {
        priority: area * weight - order as f64 * 1e-9,
        solids,
        faces,
    };
    sign.validate()?;
    Ok(sign)
}
struct Entry {
    host: String,
    settings: Settings,
    proposal: Proposal,
}
#[derive(Default)]
pub struct Book {
    entries: Vec<Entry>,
    identity_bytes: usize,
    parts: usize,
}
pub use superspec_model::theme::signs::{Plan, PlanBounds, Status};
impl Book {
    pub fn propose(
        &mut self,
        host: &str,
        settings: &Settings,
        proposal: Proposal,
    ) -> Result<(), String> {
        let parts = proposal
            .panels
            .iter()
            .map(|p| if p.sides == Sides::Both { 3 } else { 2 })
            .sum::<usize>();
        let cost = host.len()
            + proposal.key.len()
            + settings.weathering.seed.len()
            + proposal
                .panels
                .iter()
                .map(|p| {
                    p.pen.part.len() + p.design.blocks.iter().map(|b| b.text.len()).sum::<usize>()
                })
                .sum::<usize>();
        if self.entries.len() >= 4096
            || self.parts + parts > 16384
            || host.is_empty()
            || host.len() > 1024
            || proposal.key.len() > 2048
            || self.identity_bytes + cost > 4 * 1024 * 1024
        {
            return Err("Sign collection exceeds bounded host identities or entries.".into());
        }
        settings.weathering.validate()?;
        surveyed(&proposal, self.entries.len())?;
        self.identity_bytes += cost;
        self.parts += parts;
        self.entries.push(Entry {
            host: host.into(),
            settings: settings.clone(),
            proposal,
        });
        Ok(())
    }
    pub fn append(&mut self, source: &mut Self) -> Result<(), String> {
        if self.entries.len() + source.entries.len() > 4096
            || self.parts + source.parts > 16384
            || self.identity_bytes + source.identity_bytes > 4 * 1024 * 1024
        {
            return Err("Combined sign collection exceeds scene allowance.".into());
        }
        self.entries.append(&mut source.entries);
        self.identity_bytes += source.identity_bytes;
        self.parts += source.parts;
        source.parts = 0;
        source.identity_bytes = 0;
        Ok(())
    }
    pub fn shop_near(&self, host: &str, at: V3, radius: f64) -> Result<Option<&Business>, String> {
        self.shop_near_with(host, at, radius, &[])
    }
    /// Earlier committed faces win distance ties; local proposals retain their drawing order.
    pub fn shop_near_with<'a>(
        &'a self,
        host: &str,
        at: V3,
        radius: f64,
        local: &'a [Proposal],
    ) -> Result<Option<&'a Business>, String> {
        if at.iter().any(|v| !v.is_finite() || v.abs() > 10000.)
            || local.len() > 4096
            || !radius.is_finite()
            || !(0. ..=1000.).contains(&radius)
        {
            return Err("Shop lookup requires bounded finite position and radius.".into());
        }
        let mut nearest = None;
        let mut least = radius;
        for proposal in self
            .entries
            .iter()
            .filter(|e| e.host == host)
            .map(|e| &e.proposal)
            .chain(local)
        {
            if proposal.mount != Mount::Fascia {
                continue;
            }
            let Some(business) = proposal.business.as_ref() else {
                continue;
            };
            let p = proposal
                .panels
                .first()
                .ok_or("Shop proposal requires a panel.")?;
            let center = p.pen.world([p.design.width / 2., 0., p.design.height / 2.]);
            let distance = hypot(center[0] - at[0], center[1] - at[1]);
            if distance < least {
                nearest = Some(business);
                least = distance
            }
        }
        Ok(nearest)
    }
    /// Draw into caller-owned detached host meshes only after all placement decisions succeed.
    /// The callback returns this proposal's emitted triangle count; errors abort publication.
    pub fn settle(
        self,
        scenery: &Obstructions,
        budget: &mut Budget<'_>,
        mut draw: impl FnMut(&str, &Proposal, &Settings) -> Result<usize, String>,
    ) -> Result<Vec<Plan>, String> {
        let judged = self
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| surveyed(&e.proposal, i))
            .collect::<Result<Vec<_>, _>>()?;
        let verdicts = survey::survey(&judged, scenery, budget)?;
        let mut plans = Vec::with_capacity(self.entries.len());
        for ((e, sign), verdict) in self.entries.into_iter().zip(judged).zip(verdicts) {
            budget.spend(1)?;
            let p = e.proposal;
            let bounds = Bounds::points(sign.solids.iter().flat_map(survey::solid_corners), 0.);
            let status = match verdict {
                Verdict::Clear => {
                    if p.shown {
                        Status::Built
                    } else {
                        Status::Withheld
                    }
                }
                Verdict::Crowded => Status::Crowded,
                Verdict::Obstructed => Status::Obstructed,
            };
            let triangles = if matches!(status, Status::Built) {
                draw(&e.host, &p, &e.settings)?
            } else {
                0
            };
            plans.push(Plan {
                id: format!("{}/{}", e.host, p.key),
                host: e.host,
                mount: p.mount,
                form: p.form,
                lines: p
                    .panels
                    .iter()
                    .flat_map(|panel| panel.design.blocks.iter().map(|b| b.text.clone()))
                    .collect(),
                bounds: PlanBounds {
                    min: bounds.low,
                    max: bounds.high,
                },
                status,
                triangles,
            });
        }
        Ok(plans)
    }
}
