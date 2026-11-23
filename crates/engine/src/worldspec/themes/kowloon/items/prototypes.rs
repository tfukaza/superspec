//! The fixtures described as shared prototypes: what shapes an instance (its content) kept
//! apart from what is the instance's own (its colours), so ItemSpec can record every instance
//! as a placement of one shape.
//!
//! A fixture is offered here only when its instances of equal content differ by colours that
//! multiply: the prototype's roles are found by drawing it twice in probe palettes, and a
//! fixture whose colours do not factor that way fails loudly instead of being shared wrongly.
use super::{Arguments, BOLD, draw, fixture, without_render_choices};
use crate::{
    itemspec::definition::{Mount, NativeOutput, NativePrototype, Values},
    worldspec::themes::kowloon::{aircon, graphics_detail, variants},
};
use serde_json::json;
use superspec_geometry::V3;
use superspec_types::rng::SeededRng;

/// How a generator's instances share a shape.
enum Sharing {
    /// It reads no seed and its colours are stated: its parameters are its content.
    Stated(Mount),
    /// Its seed picks a catalogue entry and nothing else, and its colours are stated: an
    /// entry with the same parameters is one shape.
    Entry,
    /// As [`Sharing::Entry`], and its stream also draws the one colour named, which is the
    /// instance's own.
    Tinted(&'static str),
}

fn sharing(name: &str) -> Option<Sharing> {
    Some(match name {
        "kowloon/conduit"
        | "kowloon/security-bars"
        | "kowloon/wall-vent"
        | "kowloon/downpipe"
        | "kowloon/riser-duct"
        | "kowloon/cornice"
        | "kowloon/roof-outlet"
        | "kowloon/wall-lamp"
        | "kowloon/balcony"
        | "kowloon/balcony-door"
        | "kowloon/fan-divider" => Sharing::Stated(Mount::Wall),
        "kowloon/rail" | "kowloon/ladder" => Sharing::Stated(Mount::Free),
        "kowloon/window" | "kowloon/plants" | "kowloon/sheet-canopy" | "kowloon/sagging-tarp" => {
            Sharing::Entry
        }
        "kowloon/aircon" => Sharing::Tinted(aircon::UNIT_PAINT),
        // Not shared, because their colours do not factor into one shape and one tint:
        // `kowloon/cage` draws its paint's rust, its roof sheet's colour, a dye for each
        // garment of its washing and its caged unit's paint from the cage's own streams;
        // `kowloon/drying-rack` a dye for each garment. Sharing either means giving all but
        // one of those colours to the catalogue entry.
        _ => return None,
    })
}

/// The prototype `name` draws for `parameters`, for the generators that have one.
pub(super) fn describe(name: &str, parameters: &Values) -> Option<NativePrototype> {
    if name == "kowloon/condenser" {
        return condenser(name, parameters);
    }
    match sharing(name)? {
        Sharing::Stated(mount) => stated(parameters, mount),
        Sharing::Entry => Some(entry(name, parameters)?.0),
        Sharing::Tinted(role) => {
            let (mut prototype, budget, key) = entry(name, parameters)?;
            let colour = match name {
                "kowloon/aircon" => {
                    aircon::unit_instance(&mut SeededRng::from_text(&key), budget)?.1
                }
                _ => return None,
            };
            prototype.inputs.insert(role.to_owned(), colour);
            Some(prototype)
        }
    }
}

/// The shape of the prototype `content` describes, where it is not simply what the generator
/// draws for `content`. `generate` draws the generator for parameters.
pub(super) fn meshes(
    name: &str,
    content: &Values,
    cancelled: &dyn Fn() -> bool,
    generate: &dyn Fn(&Values) -> Result<NativeOutput, String>,
) -> Option<Result<NativeOutput, String>> {
    if name == "kowloon/condenser" {
        return Some(condenser_meshes(name, content, cancelled));
    }
    match sharing(name)? {
        Sharing::Stated(_) => None,
        Sharing::Entry => Some(entry_meshes(content, None, generate)),
        Sharing::Tinted(role) => Some(entry_meshes(content, Some(role), generate)),
    }
}

/// A fixture that takes no seed and stated colours: its parameters are its content. The
/// variant budget shapes nothing in it.
fn stated(parameters: &Values, mount: Mount) -> Option<NativePrototype> {
    // A seed would be the fixture's own: what it draws from it is not known to be shared.
    if parameters.contains_key("seed") {
        return None;
    }
    let detail = graphics_detail::parse(parameters.get(graphics_detail::PARAMETER)).ok()?;
    let mut content = without_render_choices(parameters).unwrap_or_else(|| parameters.clone());
    content.insert(graphics_detail::PARAMETER.into(), json!(detail.level()));
    Some(NativePrototype {
        content,
        inputs: Default::default(),
        mount,
    })
}

/// What stands for the seed in the content of a fixture whose seed picks a catalogue entry.
const ENTRY: &str = "entry";

/// A fixture under a variant budget whose seed picks its catalogue entry: the entry with the
/// fixture's other parameters. Also the budget and the text its stream is made from. Without
/// a catalogue every fixture is its own.
fn entry(name: &str, parameters: &Values) -> Option<(NativePrototype, variants::Budget, String)> {
    let detail = graphics_detail::parse(parameters.get(graphics_detail::PARAMETER)).ok()?;
    let budget = variants::parse(parameters.get(variants::PARAMETER))
        .ok()?
        .unwrap_or(variants::Budget::of(detail));
    let mut content = without_render_choices(parameters).unwrap_or_else(|| parameters.clone());
    if content.contains_key(ENTRY) {
        return None;
    }
    let key = Arguments::new(name, &content).seed().ok()?;
    let picked = budget.pick(&mut SeededRng::from_text(&key))?;
    content.remove("seed");
    content.insert(ENTRY.into(), json!(picked));
    content.insert(graphics_detail::PARAMETER.into(), json!(detail.level()));
    Some((
        NativePrototype {
            content,
            inputs: Default::default(),
            mount: Mount::Wall,
        },
        budget,
        key,
    ))
}

/// Draws the catalogue entry `content` states with its other parameters; where its instances
/// own the colour `role`, with that colour's shades and roles found by probing.
fn entry_meshes(
    content: &Values,
    role: Option<&'static str>,
    generate: &dyn Fn(&Values) -> Result<NativeOutput, String>,
) -> Result<NativeOutput, String> {
    let mut parameters = content.clone();
    let picked = parameters
        .remove(ENTRY)
        .and_then(|entry| entry.as_u64())
        .filter(|entry| *entry < u64::from(variants::LARGEST))
        .ok_or_else(|| {
            format!(
                "a catalogued prototype states its entry, a whole number below {}.",
                variants::LARGEST
            )
        })? as u32;
    if let Some(own) = ["seed", variants::PARAMETER]
        .into_iter()
        .find(|own| parameters.contains_key(*own))
    {
        return Err(format!("a prototype states no {own}."));
    }
    match role {
        None => variants::with_entry(picked, None, || generate(&parameters)),
        Some(role) => probed(&[role], |palette| {
            variants::with_entry(picked, Some((role, palette(role))), || {
                generate(&parameters)
            })
        }),
    }
}

/// A condenser under a variant budget is its catalogue entry in its span; its case paint is
/// the instance's. Without a catalogue every condenser is its own.
fn condenser(name: &str, parameters: &Values) -> Option<NativePrototype> {
    let detail = graphics_detail::parse(parameters.get(graphics_detail::PARAMETER)).ok()?;
    let budget = variants::parse(parameters.get(variants::PARAMETER))
        .ok()?
        .unwrap_or(variants::Budget::of(detail));
    let own = without_render_choices(parameters);
    let mut arguments = Arguments::new(name, own.as_ref().unwrap_or(parameters));
    let width = arguments.number("width", 0.95).ok()?;
    let height = arguments.number("height", 0.6).ok()?;
    let plates = arguments.boolean("plates", false).ok()?;
    let weight = arguments.bounded("weight", BOLD, 1. ..=3.).ok()?;
    let mut rng = arguments.rng().ok()?;
    let (entry, body) =
        aircon::condenser_instance(&mut rng, [-width / 2., width / 2., 0., height], budget)?;
    Some(NativePrototype {
        content: Values::from([
            (ENTRY.into(), json!(entry)),
            ("width".into(), json!(width)),
            ("height".into(), json!(height)),
            ("plates".into(), json!(plates)),
            ("weight".into(), json!(weight)),
            (graphics_detail::PARAMETER.into(), json!(detail.level())),
        ]),
        inputs: [(aircon::UNIT_PAINT.to_owned(), body)].into(),
        mount: Mount::Wall,
    })
}

fn condenser_meshes(
    name: &str,
    content: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let detail = graphics_detail::parse(content.get(graphics_detail::PARAMETER))?;
    let own = without_render_choices(content);
    let mut arguments = Arguments::new(name, own.as_ref().unwrap_or(content));
    let entry = arguments.integer(ENTRY, 0, 0..=variants::LARGEST as usize - 1)? as u32;
    let width = arguments.number("width", 0.95)?;
    let height = arguments.number("height", 0.6)?;
    let options = aircon::Options {
        plates: arguments.boolean("plates", false)?,
        louvred: false,
        weight: arguments.bounded("weight", BOLD, 1. ..=3.)?,
        variants: variants::Budget::Unlimited,
        entry: None,
    };
    arguments.finish()?;
    let span = [-width / 2., width / 2., 0., height];
    probed(&[aircon::UNIT_PAINT], |palette| {
        graphics_detail::with_requested(detail, || {
            draw(cancelled, |mesh| {
                aircon::condenser_of_entry(
                    &mut fixture(mesh, "condenser")?,
                    span,
                    entry,
                    palette(aircon::UNIT_PAINT),
                    options,
                )
                .map(|_| ())
            })
        })
    })
}

/// What every role is drawn in for the first probe: dark enough that a shade lighter than
/// its colour is not clipped at white.
const HALF: f64 = 0.5;
/// What role `index` is drawn in for the second: a tenth or more from every other role's in
/// each channel.
fn sentinel(index: usize) -> V3 {
    let step = |index: usize| 0.1 * index as f64;
    [
        0.1 + step(index),
        0.85 - step(index),
        0.1 + step(index * 3 % 8),
    ]
}
/// A render mesh keeps vertex colours to a thousandth (`itemspec::mesh`), so a shade read
/// back from a probe is within this of its colour times the role's ratio.
const READING: f64 = 1.5e-3;

/// Draws a fixture twice, in an even grey and in one sentinel colour per role (`roles` in
/// name order), and returns the first drawing with each vertex's role and its shade of that
/// role. A vertex both drawings colour alike is the fixture's own; one that changed by exactly
/// a role's ratio is a shade of that role. Anything else is a colour that does not multiply.
fn probed(
    roles: &[&str],
    paint: impl Fn(&dyn Fn(&str) -> V3) -> Result<NativeOutput, String>,
) -> Result<NativeOutput, String> {
    if roles.len() > 8 {
        return Err("A prototype has at most 8 colour roles.".into());
    }
    let mut output = paint(&|_| [HALF; 3])?;
    let marked = paint(&|role| {
        roles
            .iter()
            .position(|known| *known == role)
            .map_or([HALF; 3], sentinel)
    })?;
    if output.meshes.len() != marked.meshes.len() {
        return Err("A prototype's shape depends on its colours.".into());
    }
    for (mesh, marked) in output.meshes.iter_mut().zip(&marked.meshes) {
        if mesh.positions != marked.positions
            || mesh.indices != marked.indices
            || mesh.material_slot != marked.material_slot
            || mesh.colors.len() != marked.colors.len()
        {
            return Err("A prototype's shape depends on its colours.".into());
        }
        mesh.roles = vec![0; mesh.colors.len()];
        for (at, (colour, other)) in mesh.colors.iter_mut().zip(&marked.colors).enumerate() {
            if (0..3).all(|k| (colour[k] - other[k]).abs() <= 1e-9) {
                continue;
            }
            let mut matching = (0..roles.len()).filter(|&index| {
                let ratio = sentinel(index).map(|v| v / HALF);
                (0..3).all(|k| (colour[k] * ratio[k] - other[k]).abs() <= READING)
            });
            let (Some(role), None) = (matching.next(), matching.next()) else {
                return Err(format!(
                    "A colour of slot {} is not a shade of one role: {:?} in grey, {:?} marked.",
                    mesh.material_slot,
                    &colour[..3],
                    &other[..3]
                ));
            };
            mesh.roles[at] = role as u8 + 1;
            for channel in colour.iter_mut().take(3) {
                *channel /= HALF;
            }
        }
    }
    Ok(output)
}
