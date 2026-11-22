//! Independent host emission followed by source-ordered sign and span assembly.
use crate::worldspec::themes::kowloon::{
    building::{
        self,
        materials::{Appearance, Settings},
    },
    mesh::{Mesh as Drawing, Mode, OwnedMesh},
    signs::book::Book,
};
use superspec_geometry::obstructions::Budget;
use superspec_types::architecture::{Node, V3};

pub(crate) struct Drawn {
    pub survey: Vec<V3>,
    pub drawing: OwnedMesh,
    pub signs: Book,
    pub work: usize,
}
fn draw(
    plan: &building::Plan,
    index: usize,
    appearance: &Appearance,
    rule: &Node,
    settings: &Node,
    allowance: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<Drawn, String> {
    let identity = format!(
        "{}/{}",
        settings.text("seed").unwrap(),
        plan.sites()[index].host
    );
    let mut look = Settings {
        seed: &identity,
        grime: rule.number("grime", settings.number("grime", 0.4)),
        wear: rule.number("wear", settings.number("wear", 0.2)),
        rust: rule.number(
            "rust",
            settings.number("rust", rule.number("wear", settings.number("wear", 0.2))),
        ),
        density: rule.number("texture-density", 96.),
        detail: 2,
    };
    let mut budget = Budget::new(allowance, cancelled)?;
    let reference_corner = rule.text("model") == Some("reference-corner");
    let emit = |mesh: &mut Drawing<'_>, look: &Settings<'_>, budget: &mut Budget<'_>| {
        if reference_corner {
            appearance.draw_reference_corner(&plan.sites()[index], mesh, look)
        } else {
            appearance.draw(plan, index, mesh, look, budget, cancelled)
        }
    };
    let mut sketch = Drawing::new(Mode::Survey, 2_000_000, cancelled)?;
    emit(&mut sketch, &look, &mut budget)?;
    budget.spend(sketch.survey.len())?;
    look.detail = match settings.text("detail") {
        Some("low") => 0,
        Some("high") => 2,
        _ => 1,
    };
    let mut drawing = Drawing::new(Mode::Render, 2_000_000, cancelled)?;
    let signs = emit(&mut drawing, &look, &mut budget)?;
    Ok(Drawn {
        survey: sketch.into_survey(),
        drawing: drawing.into_owned(),
        signs,
        work: allowance - budget.remaining(),
    })
}

pub(crate) fn run(
    plan: &building::Plan,
    rules: &[(usize, Node)],
    appearances: &[Appearance],
    settings: &Node,
    allowance: usize,
    cancelled: &(dyn Fn() -> bool + Sync),
    requested_workers: usize,
) -> Result<Vec<Drawn>, String> {
    #[cfg(all(target_arch = "wasm32", not(feature = "wasm-threads")))]
    let _ = requested_workers;
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Two bounded detached drawings may be in flight. Shared scene inputs remain immutable.
        let workers = std::thread::available_parallelism()
            .map_or(1, usize::from)
            .min(2)
            .min(requested_workers)
            .min(rules.len());
        let indices = (0..rules.len()).collect::<Vec<_>>();
        let results = crate::runtime::dispatch::selected(&indices, workers, &|i| {
            draw(
                plan,
                i,
                &appearances[i],
                &rules[i].1,
                settings,
                allowance,
                cancelled,
            )
        });
        let mut ordered: Vec<Option<Drawn>> = (0..rules.len()).map(|_| None).collect();
        for result in results {
            for (i, drawing) in result.map_err(|_| "Building worker panicked.")? {
                ordered[i] = Some(drawing?);
            }
        }
        ordered
            .into_iter()
            .map(|d| d.ok_or_else(|| "Building worker omitted a host.".into()))
            .collect()
    }
    #[cfg(all(target_arch = "wasm32", not(feature = "wasm-threads")))]
    {
        rules
            .iter()
            .enumerate()
            .map(|(i, (_, rule))| {
                draw(
                    plan,
                    i,
                    &appearances[i],
                    rule,
                    settings,
                    allowance,
                    cancelled,
                )
            })
            .collect()
    }
    #[cfg(all(target_arch = "wasm32", feature = "wasm-threads"))]
    {
        let lanes = requested_workers.min(2).min(rules.len());
        let indices = (0..rules.len()).collect::<Vec<_>>();
        let results = crate::runtime::dispatch::lanes(&indices, lanes, &|i| {
            draw(
                plan,
                i,
                &appearances[i],
                &rules[i].1,
                settings,
                allowance,
                cancelled,
            )
        });
        let mut ordered: Vec<Option<Drawn>> = (0..rules.len()).map(|_| None).collect();
        for lane in results {
            for (i, drawing) in lane {
                ordered[i] = Some(drawing?);
            }
        }
        ordered
            .into_iter()
            .map(|d| d.ok_or_else(|| "Building worker omitted a host.".into()))
            .collect()
    }
}
