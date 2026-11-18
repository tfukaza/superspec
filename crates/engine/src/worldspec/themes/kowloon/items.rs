//! The Kowloon fixtures as ItemSpec native generators, so item definitions can place and
//! compose them. Each generator draws the same fixture code the facades use, unweathered,
//! into a render mesh, and hands it over as ItemSpec meshes: one per material slot.
//!
//! Item coordinates follow ItemSpec's panel convention: X runs right as seen from the
//! front, Z up, and the front faces -Y (the wall plane is y = 0, the fixture stands at
//! y < 0). Kowloon's facade frame (X right, Y outward) is mapped onto it by a reflection of
//! Y, which the fixture pens already support. Roof and shop pieces are drawn in Kowloon's
//! roof and room frames, which are not reflected: their street or room side already faces
//! -Y.
//!
//! Fixtures that always route metal through Kowloon's weathering receivers are drawn with
//! zero raster density; their receivers are never baked here, so the paint each receiver
//! recorded is restored onto its vertex colors instead.
//!
//! Every generator reads its parameters by name with a default and refuses unknown ones.
//! Colors are `#rrggbb` sRGB text; lengths are meters; angles are radians. A `seed` (text
//! or number) selects the per-instance variety of seeded fixtures.
use super::{
    balconies, balcony_props, doors, facade_details,
    fixture::Fixture,
    ground, household,
    kit::Kit,
    laundry,
    massing::Rect,
    mesh::{Mesh, Mode},
    metal::Binding,
    palette::srgb,
    pen::Pen,
    pipes, roof_access, rooftop, shop, shutters, signs, tarps, timber, water_tanks, weathered,
    windows,
};
use crate::itemspec::definition::{ItemMesh, NativeGenerators, NativeOutput, Values};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};
use superspec_geometry::{Matrix, V3};
use superspec_types::rng::SeededRng;

/// Bumped whenever a generator's output changes for the same parameters.
const VERSION: &str = "1";
/// Most triangles one native fixture may draw.
const MAXIMUM_TRIANGLES: usize = 200_000;

/// Reads one generator's parameters with defaults, refusing unknown names.
pub struct Arguments<'a> {
    name: &'a str,
    values: &'a Values,
    known: Vec<&'static str>,
}
impl<'a> Arguments<'a> {
    pub fn new(name: &'a str, values: &'a Values) -> Self {
        Self {
            name,
            values,
            known: vec![],
        }
    }
    pub fn number(&mut self, key: &'static str, default: f64) -> Result<f64, String> {
        Ok(self.optional_number(key)?.unwrap_or(default))
    }
    pub fn optional_number(&mut self, key: &'static str) -> Result<Option<f64>, String> {
        self.known.push(key);
        match self.values.get(key) {
            None => Ok(None),
            Some(value) => value
                .as_f64()
                .filter(|v| v.is_finite() && v.abs() <= 1e4)
                .map(Some)
                .ok_or_else(|| format!("{}: {key} must be a bounded number.", self.name)),
        }
    }
    /// A number that must lie in `range`.
    pub fn bounded(
        &mut self,
        key: &'static str,
        default: f64,
        range: std::ops::RangeInclusive<f64>,
    ) -> Result<f64, String> {
        let value = self.number(key, default)?;
        if range.contains(&value) {
            Ok(value)
        } else {
            Err(format!(
                "{}: {key} must lie in {}..{}.",
                self.name,
                range.start(),
                range.end()
            ))
        }
    }
    /// A whole number that must lie in `range`.
    pub fn integer(
        &mut self,
        key: &'static str,
        default: usize,
        range: std::ops::RangeInclusive<usize>,
    ) -> Result<usize, String> {
        let value = self.number(key, default as f64)?;
        if value.fract() == 0. && value >= 0. && range.contains(&(value as usize)) {
            Ok(value as usize)
        } else {
            Err(format!(
                "{}: {key} must be a whole number in {}..{}.",
                self.name,
                range.start(),
                range.end()
            ))
        }
    }
    pub fn boolean(&mut self, key: &'static str, default: bool) -> Result<bool, String> {
        self.known.push(key);
        match self.values.get(key) {
            None => Ok(default),
            Some(value) => value
                .as_bool()
                .ok_or_else(|| format!("{}: {key} must be a boolean.", self.name)),
        }
    }
    pub fn text(&mut self, key: &'static str, default: &str) -> Result<String, String> {
        Ok(self
            .optional_text(key)?
            .unwrap_or_else(|| default.to_owned()))
    }
    pub fn optional_text(&mut self, key: &'static str) -> Result<Option<String>, String> {
        self.known.push(key);
        match self.values.get(key) {
            None => Ok(None),
            Some(Value::String(text)) if text.len() <= 256 => Ok(Some(text.clone())),
            Some(_) => Err(format!("{}: {key} must be a short string.", self.name)),
        }
    }
    /// An `#rrggbb` sRGB color, returned in linear RGB like Kowloon's palette.
    pub fn color(&mut self, key: &'static str, default: &str) -> Result<V3, String> {
        let text = self.text(key, default)?;
        if text.len() == 7
            && text.starts_with('#')
            && text[1..].chars().all(|c| c.is_ascii_hexdigit())
        {
            Ok(srgb(&text))
        } else {
            Err(format!("{}: {key} must be an #rrggbb color.", self.name))
        }
    }
    /// One of `options`, deserialized into the fixture's own choice type.
    pub fn choice<T: serde::de::DeserializeOwned>(
        &mut self,
        key: &'static str,
        default: &str,
        options: &[&str],
    ) -> Result<T, String> {
        let text = self.text(key, default)?;
        if !options.contains(&text.as_str()) {
            return Err(format!(
                "{}: {key} must be one of {}.",
                self.name,
                options.join(", ")
            ));
        }
        serde_json::from_value(Value::String(text))
            .map_err(|e| format!("{}: {key}: {e}", self.name))
    }
    /// The fixture's identity text: `seed` as text or number, prefixed by the generator name.
    pub fn seed(&mut self) -> Result<String, String> {
        self.known.push("seed");
        let seed = match self.values.get("seed") {
            None => "0".to_owned(),
            Some(Value::String(text)) if text.len() <= 256 => text.clone(),
            Some(Value::Number(number)) => number.to_string(),
            Some(_) => return Err(format!("{}: seed must be text or a number.", self.name)),
        };
        Ok(format!("{}/{seed}", self.name))
    }
    /// The fixture's random stream, from [`Self::seed`].
    pub fn rng(&mut self) -> Result<SeededRng, String> {
        Ok(SeededRng::from_text(&self.seed()?))
    }
    /// Fails on any parameter the generator did not read.
    pub fn finish(&self) -> Result<(), String> {
        match self
            .values
            .keys()
            .find(|key| !self.known.contains(&key.as_str()))
        {
            Some(key) => Err(format!(
                "{}: unknown parameter {key}; it reads {}.",
                self.name,
                self.known.join(", ")
            )),
            None => Ok(()),
        }
    }
}

/// The pen every wall-mounted native fixture draws with: Kowloon's facade frame reflected
/// onto item axes.
pub fn pen(part: &str) -> Result<Pen, String> {
    Pen::new(
        Matrix([[1., 0., 0.], [0., -1., 0.], [0., 0., 1.], [0.; 3]]),
        part,
    )
}
/// The unreflected pen of roof and shop pieces, moved by `offset`.
pub fn room_pen(part: &str, offset: V3) -> Result<Pen, String> {
    Pen::new(Matrix::default(), part)?.moved(offset)
}

/// Draws into a fresh render mesh and converts it.
pub fn draw(
    cancelled: &dyn Fn() -> bool,
    paint: impl FnOnce(&mut Mesh<'_>) -> Result<(), String>,
) -> Result<NativeOutput, String> {
    let mut mesh = Mesh::new(Mode::Render, MAXIMUM_TRIANGLES, cancelled)?;
    paint(&mut mesh)?;
    unbaked_paint(&mut mesh);
    Ok(NativeOutput {
        meshes: meshes(&mesh)?,
        attachments: vec![],
    })
}

/// Weathering receivers draw white and leave their paint to a bake. Nothing is baked
/// for an item, so each receiver's recorded paint tints its (shaded) vertex colors.
fn unbaked_paint(mesh: &mut Mesh<'_>) {
    let ranges: Vec<(usize, usize, V3)> = mesh
        .weathering
        .receivers
        .iter()
        .filter_map(|receiver| match receiver {
            weathered::Pending::Component(r) => {
                Some((r.first_triangle, r.triangle_count, r.settings.paint))
            }
            weathered::Pending::Prepared(r) => {
                Some((r.first_triangle, r.triangle_count, r.settings.paint))
            }
            weathered::Pending::Surface { .. } => None,
        })
        .collect();
    let mut painted = vec![false; mesh.vertices.len()];
    for (first, count, paint) in ranges {
        for corner in first * 3..(first + count) * 3 {
            let Some(&index) = mesh.indices.get(corner) else {
                break;
            };
            let index = index as usize;
            if index < painted.len() && !std::mem::replace(&mut painted[index], true) {
                let color = &mut mesh.vertices[index].color;
                *color = std::array::from_fn(|k| color[k] * paint[k]);
            }
        }
    }
}

/// A fixture scope on `mesh` with unit texture repeats.
pub fn fixture<'m, 'c>(mesh: &'m mut Mesh<'c>, part: &str) -> Result<Fixture<'m, 'c>, String> {
    on(mesh, pen(part)?)
}
/// A fixture scope on `mesh` drawn with `pen`, with unit texture repeats.
pub fn on<'m, 'c>(mesh: &'m mut Mesh<'c>, pen: Pen) -> Result<Fixture<'m, 'c>, String> {
    Ok(Fixture {
        pen,
        mesh,
        repeats: &|_| 1.,
    })
}

/// Unbaked weathering settings bound to the plain `metal`, `sheet` and `paint` slots.
/// Fixtures that refuse zero raster density get `density` 64; nothing is baked either way.
pub fn weathering(seed: &str, density: f64) -> weathered::Settings {
    weathered::Settings {
        seed: seed.into(),
        wear: 0.25,
        grime: 0.6,
        rust: 0.4,
        texture_density: density,
        metal: binding("metal"),
        sheet: binding("sheet"),
        paint: binding("paint"),
    }
}
fn binding(material: &str) -> Binding {
    Binding {
        material: material.into(),
        repeat: 1.,
    }
}
/// The procedural kit of fixtures that draw by named streams: unbaked, lettered, shipped
/// timber, seeded by `seed`. `density` is the (never baked) raster density, as in
/// [`weathering`].
pub fn kit(seed: &str, detail: u8, density: f64) -> Result<Kit, String> {
    Kit::new(
        signs::panel::Settings {
            weathering: weathering(seed, density),
            timber: timber::Settings::default(),
            lettering: true,
        },
        detail,
    )
}

type Generate = fn(&str, &Values, &dyn Fn() -> bool) -> Result<NativeOutput, String>;

/// The registry: every Kowloon fixture offered to item definitions, by name.
pub struct Natives {
    generators: BTreeMap<&'static str, Generate>,
}

/// `kowloon/aircon`: a window air conditioner on its wall bracket, one of four seeded
/// models. Anchor: bottom-centre of the casing on the wall plane; brackets and drain hang
/// below z = 0. Parameters: `drop` (m of room below the casing for its supports and drain,
/// 0.7), `seed`.
fn aircon(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut arguments = Arguments::new(name, values);
    let drop = arguments.number("drop", 0.7)?;
    let mut rng = arguments.rng()?;
    arguments.finish()?;
    draw(cancelled, |mesh| {
        super::aircon::air_conditioner(
            &mut fixture(mesh, "window-unit")?,
            0.,
            0.,
            drop,
            &mut rng,
            None,
        )
        .map(|_| ())
    })
}

/// `kowloon/condenser`: a split-system condenser on wall brackets. Anchor: bottom-centre
/// on the wall plane. Parameters: `width` (m, 0.95), `height` (m, 0.6), `seed`.
fn condenser(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut arguments = Arguments::new(name, values);
    let width = arguments.number("width", 0.95)?;
    let height = arguments.number("height", 0.6)?;
    let mut rng = arguments.rng()?;
    arguments.finish()?;
    draw(cancelled, |mesh| {
        super::aircon::condenser(
            &mut fixture(mesh, "condenser")?,
            [-width / 2., width / 2., 0., height],
            &mut rng,
            None,
        )
        .map(|_| ())
    })
}

/// `kowloon/window`: a complete window in a wall opening, with sashes, sill, optional hood
/// and the room behind the glass. Anchor: bottom-centre of the opening on the wall plane;
/// the frame spans x in [-width/2, width/2] and z in [0, height], the recess and interior lie
/// behind the wall (+Y), and the hood and opened sashes project toward -Y.
/// Parameters: `kind` (`casement` | `slider` | `timber` | `louver`, `casement`), `width`
/// (m, 1.2), `height` (m, 1.4), `frame` (#2f4f43), `trim` (#e6e2d6), `curtain` (#e8e0c8),
/// `reach` (m of clear space in front, which admits the hood at 0.3 and open sashes at
/// 0.6 + recess, 1.0), `recess` (m, 0.3), `hood` (true), `lit` (false), `boarded` (false),
/// `fixed` (never opened, false), `transom` (optional fraction of the height),
/// `textured` (shop-timber atlas on timber windows, true), `seed` (open sashes, curtains).
fn window(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let kind = a.choice(
        "kind",
        "casement",
        &["casement", "slider", "timber", "louver"],
    )?;
    let width = a.number("width", 1.2)?;
    let height = a.number("height", 1.4)?;
    let look = windows::Look {
        weathering: None,
        kind,
        frame: a.color("frame", "#2f4f43")?,
        trim: a.color("trim", "#e6e2d6")?,
        curtain: a.color("curtain", "#e8e0c8")?,
        reach: a.number("reach", 1.)?,
        hood: a.boolean("hood", true)?,
        lit: a.boolean("lit", false)?,
        boarded: a.boolean("boarded", false)?,
        fixed: a.boolean("fixed", false)?,
        recess: a.number("recess", 0.3)?,
        transom_ratio: a.optional_number("transom")?,
        lower_pane_bars: None,
        timber_textured: a.boolean("textured", true)?,
    };
    let mut rng = a.rng()?;
    a.finish()?;
    draw(cancelled, |mesh| {
        windows::window_sized(
            &mut fixture(mesh, "window")?,
            [-width / 2., 0., width, height],
            &look,
            &mut rng,
        )
    })
}

/// `kowloon/security-bars`: a welded security grille over an opening. Anchor: bottom-centre
/// of the guarded opening on the wall plane. Parameters: `pattern` (`bars` | `grid` |
/// `diamond`, `bars`), `width` (m, 1.2), `height` (m, 1.4), `color` (#2a2c2e).
fn security_bars(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let pattern = a.choice("pattern", "bars", &["bars", "grid", "diamond"])?;
    let width = a.number("width", 1.2)?;
    let height = a.number("height", 1.4)?;
    let color = a.color("color", "#2a2c2e")?;
    a.finish()?;
    draw(cancelled, |mesh| {
        windows::security_bars_sized(
            &mut fixture(mesh, "security-bars")?,
            [-width / 2., 0., width, height],
            pattern,
            color,
            None,
        )
    })
}

/// Facade detail settings for a lone vent or outlet: detail bolts, concrete trim.
fn details(name: &str, detail: usize) -> facade_details::Settings {
    facade_details::Settings {
        weathering: weathering(name, 64.),
        concrete: binding("concrete"),
        detail,
        trim: [0.8, 0.77, 0.65],
    }
}

/// `kowloon/wall-vent`: a louvered metal wall vent. Anchor: bottom-centre on the wall
/// plane. Parameters: `width` (m, 0.38), `height` (m, 0.23), `detail` (0–2; corner bolts
/// from 1, 1).
fn wall_vent(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let width = a.number("width", 0.38)?;
    let height = a.number("height", 0.23)?;
    let detail = a.integer("detail", 1, 0..=2)?;
    a.finish()?;
    let plan = facade_details::Plan {
        bands: vec![],
        vents: vec![Rect {
            u0: -width / 2.,
            u1: width / 2.,
            v0: 0.,
            v1: height,
        }],
        outlets: vec![],
        ledges: vec![],
    };
    draw(cancelled, |mesh| {
        facade_details::draw(
            &mut fixture(mesh, "wall-vent")?,
            &plan,
            &details(name, detail),
            "wall-vent",
            &|_| Ok([None; 2]),
        )
    })
}

/// `kowloon/roof-outlet`: a parapet rainwater outlet with its lipped spout. Anchor:
/// bottom-centre of the opening on the wall plane. Parameters: `width` (m, 0.28), `height`
/// (m, 0.215).
fn roof_outlet(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let width = a.number("width", 0.28)?;
    let height = a.number("height", 0.215)?;
    a.finish()?;
    let plan = facade_details::Plan {
        bands: vec![],
        vents: vec![],
        outlets: vec![Rect {
            u0: -width / 2.,
            u1: width / 2.,
            v0: 0.,
            v1: height,
        }],
        ledges: vec![],
    };
    draw(cancelled, |mesh| {
        facade_details::draw(
            &mut fixture(mesh, "roof-outlet")?,
            &plan,
            &details(name, 1),
            "roof-outlet",
            &|_| Ok([None; 2]),
        )
    })
}

/// `kowloon/riser-duct`: the industrial sheet-steel riser duct with its offset flanges,
/// brackets and access hatch, in the `vent-metal` and `vent-edge` slots. Anchor:
/// bottom-centre on the wall plane; the duct is 0.78 m wide and stands about 1 m out.
/// Parameters: `height` (m from the lower to the upper wall connection, 3–20, 4.8).
fn riser_duct(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let height = a.bounded("height", 4.8, 3. ..=20.)?;
    a.finish()?;
    draw(cancelled, |mesh| {
        super::facade::services::draw_duct(&mut fixture(mesh, "riser-duct")?, 0., 0., height)
    })
}

/// Unweathered pipe options shared by the pipe generators.
fn pipe_options(a: &mut Arguments<'_>) -> Result<(f64, f64, pipes::PipeSystem), String> {
    let radius = a.number("radius", 0.05)?;
    let standoff = a.number("standoff", 0.03)?;
    let system = pipes::PipeSystem::new(pipes::Options {
        radius,
        color: a.color("color", "#3a3c3a")?,
        sides: a.integer("sides", 16, 8..=64)?,
        variant: "pipe-standard".into(),
        weathering: None,
    })?;
    Ok((radius, standoff, system))
}

/// `kowloon/downpipe`: a vertical drainpipe with socketed couplings and wall straps.
/// Anchor: the foot of the pipe, at x = 0 on the wall plane; the pipe axis stands
/// `radius + standoff` in front of the wall. Parameters: `height` (m, 3), `radius` (m,
/// 0.05), `standoff` (m of gap to the wall, 0.03), `color` (#3a3c3a), `sides` (8–64, 16),
/// `straps` (wall straps every 1.5 m, true).
fn downpipe(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let height = a.number("height", 3.)?;
    let (radius, standoff, system) = pipe_options(&mut a)?;
    let straps = a.boolean("straps", true)?;
    a.finish()?;
    let y = radius + standoff;
    draw(cancelled, |mesh| {
        system.run(
            &mut fixture(mesh, "downpipe")?,
            &[[0., y, height], [0., y, 0.]],
            straps.then_some(0.),
        )
    })
}

/// `kowloon/pipe-elbow`: an L-shaped wall pipe run that rises and then turns along the
/// wall through a socketed elbow. Anchor: the foot of the rise, at x = 0 on the wall plane.
/// Parameters: `rise` (m, 1.5), `run` (m along X after the elbow, negative runs left, 1),
/// `radius` (m, 0.05), `standoff` (m, 0.03), `color` (#3a3c3a), `sides` (8–64, 16),
/// `straps` (true).
fn pipe_elbow(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let rise = a.number("rise", 1.5)?;
    let run = a.number("run", 1.)?;
    let (radius, standoff, system) = pipe_options(&mut a)?;
    let straps = a.boolean("straps", true)?;
    a.finish()?;
    let y = radius + standoff;
    draw(cancelled, |mesh| {
        system.run(
            &mut fixture(mesh, "pipe-elbow")?,
            &[[0., y, 0.], [0., y, rise], [run, y, rise]],
            straps.then_some(0.),
        )
    })
}

/// `kowloon/balcony`: a cantilevered concrete balcony slab with an open-backed parapet.
/// Anchor: the centre of the slab's back edge on the wall plane, at the slab's top (z = 0).
/// Parameters: `width` (m along the wall, 2.4), `depth` (m out from the wall, 1.1), `splay`
/// (m each front corner widens sideways, 0), `thickness` (m of slab, 0.12), `parapet`
/// (true), `parapet_height` (m, 0.85), `color` (#d6e4d4), `trim` (cap, #e6e2d6).
fn balcony(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let width = a.number("width", 2.4)?;
    let depth = a.number("depth", 1.1)?;
    let splay = a.number("splay", 0.)?;
    let thickness = a.number("thickness", 0.12)?;
    let parapet = a.boolean("parapet", true)?;
    // `steel`: the reference shop house's railing (three green rails on posts, a low concrete
    // upstand) in place of the solid parapet.
    let rail = a.text("rail", "parapet")?;
    if !["parapet", "steel"].contains(&rail.as_str()) {
        return Err(format!("{name}: rail must be one of parapet, steel."));
    }
    let steel = a.color("rail_color", "#3f554b")?;
    let height = a.number("parapet_height", 0.85)?;
    let color = a.color("color", "#d6e4d4")?;
    let trim = a.color("trim", "#e6e2d6")?;
    a.finish()?;
    let outline = [
        [-width / 2., 0., 0.],
        [width / 2., 0., 0.],
        [width / 2. + splay, depth, 0.],
        [-width / 2. - splay, depth, 0.],
    ];
    let settings = balconies::Settings {
        weathering: weathering(name, 64.),
        wall: binding("wall"),
        concrete: binding("concrete"),
    };
    draw(cancelled, |mesh| {
        let d = &mut fixture(mesh, "balcony")?;
        balconies::slab(d, &outline, -thickness, 0., color, "concrete")?;
        if parapet && rail == "steel" {
            steel_rail(d, mesh_weathering(name), &outline, steel, trim)?;
        } else if parapet {
            balconies::parapet(d, &outline, height, color, trim, &settings, "balcony")?;
        }
        Ok(())
    })
}

/// The unbaked weathering of a native fixture, with the fixture's name as its seed.
fn mesh_weathering(name: &str) -> weathered::Settings {
    weathering(name, 0.)
}

/// The reference shop house's balcony railing around the three open sides of a four-point
/// outline (back left, back right, front right, front left): a low concrete upstand with three
/// steel rails on posts, mitred at the corners.
fn steel_rail(
    d: &mut Fixture<'_, '_>,
    settings: weathered::Settings,
    outline: &[V3; 4],
    steel: V3,
    cap: V3,
) -> Result<(), String> {
    let original = d.pen.clone();
    let path = [outline[0], outline[3], outline[2], outline[1]];
    let segments: Vec<(V3, f64)> = path
        .windows(2)
        .map(|p| {
            let [x, y] = [p[1][0] - p[0][0], p[1][1] - p[0][1]];
            let length = x.hypot(y);
            ([x / length, y / length, 0.], length)
        })
        .collect();
    let turn = |a: V3, b: V3| (a[0] * b[1] - a[1] * b[0]).atan2(a[0] * b[0] + a[1] * b[1]);
    for (i, &(tangent, length)) in segments.iter().enumerate() {
        let miters = [
            i.checked_sub(1)
                .map(|j| (turn(segments[j].0, tangent) / 2.).tan()),
            segments
                .get(i + 1)
                .map(|&(next, _)| -(turn(tangent, next) / 2.).tan()),
        ];
        d.pen = original.facing(path[i], tangent, [-tangent[1], tangent[0], 0.])?;
        d.cuboid("concrete", [0., -0.075, 0.], [length, 0., 0.3], cap, "")?;
        d.pen = d.pen.with_weathering(
            d.mesh,
            settings.clone(),
            super::metal::Kind::Cage,
            None,
            vec!["metal".into()],
        )?;
        let start = -0.035 * miters[0].unwrap_or(0.);
        let end = length - 0.035 * miters[1].unwrap_or(0.);
        for z in [0.59, 0.83, 1.08] {
            d.tube(
                "metal",
                &[[start, -0.035, z], [end, -0.035, z]],
                0.023,
                steel,
                10,
            )?;
        }
        let posts = (length / 1.4).ceil() as usize;
        for j in 0..=posts {
            let x = start + (end - start) * j as f64 / posts as f64;
            d.tube(
                "metal",
                &[[x, -0.035, 0.28], [x, -0.035, 1.11]],
                0.026,
                steel,
                10,
            )?;
        }
    }
    d.pen = original;
    Ok(())
}

/// `kowloon/board-sign`: a lettered sheet sign hung from two outriggers, as on the reference shop
/// house. Anchor: the pier line on the wall plane at the board's foot; the board projects
/// `reach` from the wall (or stands at `angle` degrees to it, turning about the vertical axis
/// at its fixing). Parameters: `text` (main lettering, `興隆五金`), `width` (m, 0.94), `height`
/// (m, 3.3), `red` (true: ivory on oxblood; false: oxblood on ivory), `horizontal` (reads left to
/// right, false: top to bottom), `reach` (m of outrigger, 0.8), `angle` (degrees, 0), `age`
/// (0–1, 0.7), `anchor` (m along the board from its left edge to where the outriggers meet the
/// wall, 0), `end` (m along it where they end; default the board's far edge, or its far end at
/// an angle), `seed` (weathering).
fn board_sign(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    use signs::{
        Construction, Outline,
        lettering::{Direction, Face},
        panel,
    };
    let mut a = Arguments::new(name, values);
    let text = a.text("text", "興隆五金")?;
    let width = a.number("width", 0.94)?;
    let height = a.number("height", 3.3)?;
    let red = a.boolean("red", true)?;
    let horizontal = a.boolean("horizontal", false)?;
    let reach = a.number("reach", 0.8)?;
    let angle = a.number("angle", 0.)?.to_radians();
    let anchor = a.number("anchor", 0.)?;
    let end_at = a.number("end", width)?;
    let age = a.number("age", 0.7)?;
    let key = a.seed()?;
    a.finish()?;
    let ivory = srgb("#dfcfaa");
    let oxblood = srgb("#793f32");
    let (ground, ink) = if red {
        (oxblood, ivory)
    } else {
        (ivory, oxblood)
    };
    let hardware = panel::Hardware {
        slot: "metal".into(),
        tint: srgb("#504d3d"),
        weathering: None,
    };
    let settings = sign_settings(&key);
    draw(cancelled, |mesh| {
        let root = pen("sign")?;
        let (sin, cos) = angle.sin_cos();
        // The board's own frame: its face along the direction `angle` off the wall, its body
        // standing `reach` out from the wall.
        let pen = root.facing([0., reach, 0.], [cos, sin, 0.], [-sin, cos, 0.])?;
        panel::draw(
            &mut on(mesh, pen)?,
            &panel::Design {
                width,
                height,
                depth: 0.045,
                construction: Construction::Sheet,
                outline: Outline::Rectangle,
                ground,
                border: Some(ink),
                stack: panel::Stack::Down,
                blocks: vec![panel::Block {
                    text: text.clone(),
                    face: Face::Kai,
                    direction: if horizontal {
                        Direction::Ltr
                    } else {
                        Direction::Ttb
                    },
                    ink,
                    share: 1.,
                    shadow: None,
                }],
                age,
                phase: SeededRng::from_text(&key).float(0., 100.),
            },
            panel::Sides::Both,
            &hardware,
            &settings,
        )?;
        let d = &mut on(mesh, root)?;
        for z in [0.12, height - 0.12] {
            if angle != 0. {
                let end = [width * cos, reach + width * sin, z];
                d.tube(
                    "metal",
                    &[[anchor, 0.02, z], [anchor, reach, z], end],
                    0.027,
                    hardware.tint,
                    10,
                )?;
                d.tube(
                    "metal",
                    &[[anchor + 0.5, 0.02, z + 0.25], end],
                    0.018,
                    hardware.tint,
                    8,
                )?;
                continue;
            }
            d.tube(
                "metal",
                &[[anchor, 0.02, z], [anchor, reach, z], [end_at, reach, z]],
                0.023,
                hardware.tint,
                8,
            )?;
            let brace = if horizontal {
                anchor
            } else if anchor > width / 2. {
                width - 0.05
            } else {
                0.05
            };
            d.tube(
                "metal",
                &[[anchor, 0.02, z + 0.3], [brace, reach, z]],
                0.018,
                hardware.tint,
                8,
            )?;
        }
        Ok(())
    })
}

/// `kowloon/anti-climb`: a semicircular steel climbing barrier bolted to the wall.
/// Anchor: bottom-centre on the wall plane (the fan's hub sits at z = radius + 0.04).
/// Parameters: `radius` (m, 0.05–5, 0.5), `color` (#e6e2d6).
fn anti_climb(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let radius = a.number("radius", 0.5)?;
    let color = a.color("color", "#e6e2d6")?;
    a.finish()?;
    draw(cancelled, |mesh| {
        balconies::anti_climb(
            &mut fixture(mesh, "anti-climb")?,
            0.,
            radius + 0.04,
            radius,
            color,
            &weathering(name, 0.),
        )
    })
}

const PLANTS: [&str; 3] = ["rubber-plant", "trailing-pothos", "snake-plant"];

/// `kowloon/plant`: one potted plant from the shipped balcony plant meshes. Anchor: the
/// bottom-centre of the pot. Parameters: `model` (`rubber-plant` | `trailing-pothos` |
/// `snake-plant`, `rubber-plant`), `scale` (1), `rotation` (radians about Z, 0), `textured`
/// (balcony-plants atlas, true).
fn plant(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let model = a.choice("model", "rubber-plant", &PLANTS)?;
    let scale = a.number("scale", 1.)?;
    let rotation = a.number("rotation", 0.)?;
    let textured = a.boolean("textured", true)?;
    a.finish()?;
    draw(cancelled, |mesh| {
        balcony_props::plant(
            &mut fixture(mesh, "plant")?,
            model,
            [0.; 3],
            scale,
            rotation,
            textured,
        )
    })
}

/// `kowloon/plants`: a seeded row of potted plants along a ledge. Anchor: the centre of
/// the row on the wall plane, pots standing on z = 0. Parameters: `width` (m, 1.5),
/// `offset` (m of the pots in front of the wall, 0.2), `textured` (true), `seed`.
fn plants(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let width = a.number("width", 1.5)?;
    let offset = a.number("offset", 0.2)?;
    let textured = a.boolean("textured", true)?;
    let mut rng = a.rng()?;
    a.finish()?;
    draw(cancelled, |mesh| {
        household::plants(
            &mut fixture(mesh, "plants")?,
            [-width / 2., width / 2.],
            offset,
            0.,
            &mut rng,
            textured,
        )
    })
}

/// `kowloon/cage`: a welded window cage with fixing hardware, optional corrugated roof and
/// seeded household contents. Anchor: bottom-centre on the wall plane. Parameters: `width`
/// (m, 1.9), `height` (m, 1.6), `depth` (m out from the wall, 0.65), `color` (#7fb0c8),
/// `stocked` (contents, true), `textured` (plant and cloth atlases, true), `seed`.
fn cage(name: &str, values: &Values, cancelled: &dyn Fn() -> bool) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let width = a.number("width", 1.9)?;
    let cage = household::Cage {
        span: [-width / 2., width / 2.],
        heights: [0., a.number("height", 1.6)?],
        depth: a.number("depth", 0.65)?,
        color: a.color("color", "#7fb0c8")?,
        stocked: a.boolean("stocked", true)?,
    };
    let textured = a.boolean("textured", true)?;
    let mut rng = a.rng()?;
    a.finish()?;
    draw(cancelled, |mesh| {
        household::cage(&mut fixture(mesh, "cage")?, &cage, &mut rng, None, textured)
    })
}

/// `kowloon/drying-rack`: bamboo poles on two wall arms, hung with seeded washing.
/// Anchor: the centre of the arms' wall fixings, at the arm height; the washing hangs below
/// z = 0. Parameters: `width` (m between the arms, 1.5), `depth` (m of arm reach, 0.6),
/// `textured` (true), `seed`.
fn drying_rack(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let width = a.number("width", 1.5)?;
    let depth = a.number("depth", 0.6)?;
    let textured = a.boolean("textured", true)?;
    let mut rng = a.rng()?;
    a.finish()?;
    draw(cancelled, |mesh| {
        household::drying_rack(
            &mut fixture(mesh, "drying-rack")?,
            [-width / 2., width / 2.],
            0.,
            depth,
            &mut rng,
            None,
            textured,
        )
    })
}

/// `kowloon/drying-pole`: a socketed bamboo pole projecting from the wall, carrying
/// seeded hangers. Anchor: the wall socket. Parameters: `length` (m, 0.35–20, 1.6),
/// `textured` (true), `seed`.
fn drying_pole(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let length = a.number("length", 1.6)?;
    let textured = a.boolean("textured", true)?;
    let mut rng = a.rng()?;
    a.finish()?;
    draw(cancelled, |mesh| {
        household::drying_pole(
            &mut fixture(mesh, "drying-pole")?,
            0.,
            0.,
            length,
            &mut rng,
            None,
            textured,
        )
    })
}

/// `kowloon/drying-lines`: three roof drying lines between T-posts, hung with seeded
/// washing (roof frame). Anchor: the centre between the two post feet, on z = 0.
/// Parameters: `length` (m between the posts, 0.3–20, 3.5), `textured` (true), `seed`.
fn drying_lines(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let length = a.number("length", 3.5)?;
    let textured = a.boolean("textured", true)?;
    let mut rng = a.rng()?;
    a.finish()?;
    draw(cancelled, |mesh| {
        household::drying_lines(
            &mut on(mesh, room_pen("drying-lines", [0.; 3])?)?,
            [-length / 2., length / 2.],
            0.,
            &mut rng,
            None,
            textured,
        )
    })
}

/// `kowloon/awning`: a household awning, settled canvas or corrugated sheet on steel
/// brackets. Anchor: the centre of its top edge on the wall plane; it projects `depth`
/// and falls `drop`. Parameters: `width` (m, 2), `depth` (m, 0.9), `drop` (m, 0.35),
/// `canvas` (true; false gives corrugated sheet), `color` (#3f7f5f), `hem` (#e8dfc0),
/// `seed` (canvas folds).
fn awning(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let width = a.number("width", 2.)?;
    let envelope = tarps::Envelope {
        x0: -width / 2.,
        x1: width / 2.,
        z: 0.,
        depth: a.number("depth", 0.9)?,
        drop: a.number("drop", 0.35)?,
        valance: 0.,
    };
    let style = super::palette::Awning {
        canvas: a.boolean("canvas", true)?,
        base: a.color("color", "#3f7f5f")?,
        hem: a.color("hem", "#e8dfc0")?,
    };
    let key = a.seed()?;
    a.finish()?;
    draw(cancelled, |mesh| {
        household::awning(&mut fixture(mesh, "awning")?, envelope, &style, &key, None)
    })
}

/// `kowloon/tarp`: a fitted canvas shop awning with optional valance and stripes. Anchor:
/// the centre of its top edge on the wall plane. Parameters: `width` (m, 3.2), `depth` (m,
/// 1.3), `drop` (m, 0.35), `valance` (m, 0–0.6, 0.28), `color` (#3f7f5f), `hem` (#e8dfc0),
/// `stripes` (false), `stripe` (#d8a030), `stripe_width` (m, 0.24), `seed` (canvas forms).
fn tarp(name: &str, values: &Values, cancelled: &dyn Fn() -> bool) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let width = a.number("width", 3.2)?;
    let envelope = tarps::Envelope {
        x0: -width / 2.,
        x1: width / 2.,
        z: 0.,
        depth: a.number("depth", 1.3)?,
        drop: a.number("drop", 0.35)?,
        valance: a.number("valance", 0.28)?,
    };
    let tint = a.color("color", "#3f7f5f")?;
    let hem = a.color("hem", "#e8dfc0")?;
    let stripes = a.boolean("stripes", false)?;
    let pattern = tarps::Pattern {
        color: a.color("stripe", "#d8a030")?,
        width: a.number("stripe_width", 0.24)?,
    };
    let key = a.seed()?;
    a.finish()?;
    draw(cancelled, |mesh| {
        tarps::canvas(
            &mut fixture(mesh, "tarp")?,
            envelope,
            tint,
            hem,
            &key,
            None,
            stripes.then_some(pattern),
        )
    })
}

/// `kowloon/wall-lamp`: a caged bulkhead wall lamp. Anchor: bottom-centre on the wall
/// plane. No parameters.
fn wall_lamp(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    Arguments::new(name, values).finish()?;
    draw(cancelled, |mesh| {
        household::wall_lamp(
            &mut fixture(mesh, "wall-lamp")?,
            0.,
            0.,
            &weathering(name, 0.),
        )
    })
}

/// `kowloon/meter-bank`: a row of electricity meters with their rising conduits.
/// Anchor: bottom-centre of the row on the wall plane. Parameters: `count` (1–16, 4),
/// `seed` (housing shade).
fn meter_bank(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let count = a.integer("count", 4, 1..=16)?;
    let mut rng = a.rng()?;
    a.finish()?;
    let x = -(count as f64 * 0.3 - 0.06) / 2.;
    draw(cancelled, |mesh| {
        household::meter_bank(
            &mut fixture(mesh, "meter-bank")?,
            x,
            0.,
            count,
            &mut rng,
            &weathering(name, 0.),
        )
    })
}

/// `kowloon/roller-shutter`: a corrugated roller shutter with guides and head box.
/// Anchor: bottom-centre of the opening on the wall plane. Parameters: `width` (m, 1.2),
/// `height` (m to the head, 2.2), `raised` (m the curtain is rolled up from the floor, 0),
/// `color` (#8f969a).
fn roller_shutter(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let width = a.number("width", 1.2)?;
    let height = a.number("height", 2.2)?;
    let raised = a.number("raised", 0.)?;
    let color = a.color("color", "#8f969a")?;
    a.finish()?;
    draw(cancelled, |mesh| {
        shutters::roller_shutter(
            &pen("roller-shutter")?,
            mesh,
            [-width / 2., width / 2., raised, height],
            color,
            &weathering(name, 0.),
        )
    })
}

/// `kowloon/entrance`: a tenement entrance (single, double, glazed or steel door with its
/// frame, transom, threshold, hardware and posters) placed within a bay. Anchor:
/// bottom-centre of the bay on the wall plane; the seed chooses the door's design and its
/// position within the bay. Parameters: `width` (m of bay, 1.5–100, 1.6), `height` (m of
/// storey, 2.2–50, 3), `trim` (#ccc7b8), `posters` (true), `detail` (0–2, 1), `seed`.
fn entrance(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let width = a.number("width", 1.6)?;
    let height = a.number("height", 3.)?;
    let trim = a.color("trim", "#ccc7b8")?;
    let posters = a.boolean("posters", true)?;
    let detail = a.integer("detail", 1, 0..=2)?;
    let key = a.seed()?;
    a.finish()?;
    let settings = doors::Settings {
        weathering: weathering(&key, 0.),
        timber: timber::Settings::default(),
        trim,
        detail,
        posters,
    };
    let unit = doors::Unit {
        u0: -width / 2.,
        u1: width / 2.,
        height,
        key: key.clone(),
    };
    draw(cancelled, |mesh| {
        doors::entrance(
            &mut fixture(mesh, "entrance")?,
            &unit,
            &settings,
            &mut SeededRng::from_text(&key),
        )
        .map(|_| ())
    })
}

/// `kowloon/ground-bay`: one ground-floor shopfront bay: shutter, gate, entrance, open shop
/// front or service door, with its signs. Anchor: bottom-centre of the bay on the wall
/// plane. Parameters: `kind` (`shutter` | `gate` | `door` | `shop` | `service`, `shutter`),
/// `width` (m, 1.5–40, 3.6), `height` (m of storey, 2.2–50, 3.4), `frame` (#2f4f43), `trim`
/// (#ccc7b8), `shutter` (#5a6f68), `accent` (#5f8a72), `posters` (true), `signs` (draw the
/// bay's shop signs, true), `seed`.
fn ground_bay(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let kind = a.choice(
        "kind",
        "shutter",
        &["shutter", "gate", "door", "shop", "service"],
    )?;
    let width = a.number("width", 3.6)?;
    let height = a.number("height", 3.4)?;
    let style = ground::Style {
        frame: a.color("frame", "#2f4f43")?,
        trim: a.color("trim", "#ccc7b8")?,
        shutter: a.color("shutter", "#5a6f68")?,
        accent: a.color("accent", "#5f8a72")?,
    };
    let posters = a.boolean("posters", true)?;
    let signs = a.boolean("signs", true)?;
    let key = a.seed()?;
    a.finish()?;
    let kit = kit(&key, 1, 0.)?;
    let unit = doors::Unit {
        u0: -width / 2.,
        u1: width / 2.,
        height,
        key: key.clone(),
    };
    draw(cancelled, |mesh| {
        let proposals = ground::draw(
            &mut fixture(mesh, "ground-bay")?,
            kind,
            &unit,
            &ground::Settings {
                kit: &kit,
                style: &style,
                posters,
            },
            &mut SeededRng::from_text(&key),
        )?;
        if signs {
            for proposal in &proposals {
                proposal.draw(mesh, &|_| 1., kit.settings())?;
            }
        }
        Ok(())
    })
}

/// `kowloon/water-tank`: a rooftop water tank on its stand, round stainless or boxed, with
/// inlet pipework (roof frame). Anchor: bottom-centre of the stand. Parameters: `kind`
/// (`round` | `box`, `round`), `seed`.
fn water_tank(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let round = a.text("kind", "round")?;
    if round != "round" && round != "box" {
        return Err(format!("{name}: kind must be one of round, box."));
    }
    let key = a.seed()?;
    a.finish()?;
    let settings = water_tanks::Settings {
        weathering: weathering(&key, 0.),
        brushed: binding("brushed-metal"),
    };
    draw(cancelled, |mesh| {
        let d = &mut on(mesh, room_pen("water-tank", [0.; 3])?)?;
        let rng = &mut SeededRng::from_text(&key);
        if round == "round" {
            water_tanks::round_tank(d, [0.; 3], rng, &settings)
        } else {
            water_tanks::box_tank(d, [0.; 3], rng, &settings)
        }
    })
}

/// `kowloon/stair-hut`: a rooftop stair access hut with door, vents and parapet coping
/// (roof frame). Anchor: the centre of its footprint at roof level. Parameters: `width` (m,
/// 1.6–20, 2.4), `depth` (m, 1.5–20, 2.8), `height` (m, 2.3–10, 2.3), `color` (wall tint,
/// #a9c4a8), `seed` (cladding).
fn stair_hut(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let width = a.number("width", 2.4)?;
    let depth = a.number("depth", 2.8)?;
    let height = a.number("height", 2.3)?;
    let wall_tint = a.color("color", "#a9c4a8")?;
    let key = a.seed()?;
    a.finish()?;
    let settings = roof_access::Settings {
        weathering: weathering(&key, 64.),
        wall: binding("wall"),
        concrete: binding("concrete"),
        wall_tint,
        lot_key: key.clone(),
        masonry: None,
    };
    draw(cancelled, |mesh| {
        roof_access::stair_hut(
            &mut on(mesh, room_pen("stair-hut", [0.; 3])?)?,
            roof_access::Plot {
                x0: -width / 2.,
                x1: width / 2.,
                y0: -depth / 2.,
                y1: depth / 2.,
            },
            height,
            &settings,
        )
    })
}

/// `kowloon/aerial`: a rooftop TV aerial mast with one or two seeded arrays (roof frame).
/// Anchor: the mast foot. Parameters: `height` (m, 0.9–10, 2.4), `seed`.
fn aerial(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let height = a.bounded("height", 2.4, 0.9..=10.)?;
    let key = a.seed()?;
    a.finish()?;
    let kit = kit(&key, 1, 0.)?;
    draw(cancelled, |mesh| {
        rooftop::aerial(
            &mut on(mesh, room_pen("aerial", [0.; 3])?)?,
            [0.; 3],
            height,
            &mut SeededRng::from_text(&key),
            &kit,
        )
    })
}

/// `kowloon/rooftop-shack`: a corrugated-sheet rooftop squatter shack with door, window and
/// lean-to roof, seeded color (roof frame). Anchor: the centre of its footprint at roof
/// level; the door wall faces -Y. Parameters: `width` (m, 1–10, 2.8), `depth` (m, 1–10,
/// 2.5), `height` (m, 1.8–4, 2.1), `seed`.
fn rooftop_shack(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let width = a.bounded("width", 2.8, 1. ..=10.)?;
    let depth = a.bounded("depth", 2.5, 1. ..=10.)?;
    let height = a.bounded("height", 2.1, 1.8..=4.)?;
    let key = a.seed()?;
    a.finish()?;
    let kit = kit(&key, 1, 0.)?;
    draw(cancelled, |mesh| {
        rooftop::shack(
            &mut on(mesh, room_pen("rooftop-shack", [0.; 3])?)?,
            roof_access::Plot {
                x0: -width / 2.,
                x1: width / 2.,
                y0: -depth / 2.,
                y1: depth / 2.,
            },
            height,
            &mut SeededRng::from_text(&key),
            &kit,
        )
    })
}

/// `kowloon/rooftop`: a complete tenement roof's equipment: stair hut, water tanks, shack,
/// drying lines, plants, vents, aerials, perimeter fence and hoarding sign, all reserved
/// on one seeded deck (roof frame; the deck itself is not drawn). Anchor: the centre of the
/// deck at roof level; the street edge is at y = -depth/2. Parameters: `width` (m, 1–40,
/// 10), `depth` (m, 1–44, 11), `parapet` (m, 0–3, 0.65), `ceiling` (m of clear height for
/// aerials, 0–20, 4.8), `perimeter` (fixtures on the parapet, true), `color` (wall tint,
/// #a9c4a8), `detail` (0–2, 2), `textured` (plant and cloth atlases, true), `seed`.
fn rooftop_deck(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let width = a.number("width", 10.)?;
    let depth = a.number("depth", 11.)?;
    let parapet = a.number("parapet", 0.65)?;
    let ceiling = a.number("ceiling", 4.8)?;
    let perimeter = a.boolean("perimeter", true)?;
    let wall_tint = a.color("color", "#a9c4a8")?;
    let detail = a.integer("detail", 2, 0..=2)?;
    let textured = a.boolean("textured", true)?;
    let key = a.seed()?;
    a.finish()?;
    // The stair hut's masonry refuses zero density; signs still draw vertex artwork.
    let kit = kit(&key, detail as u8, 64.)?;
    let signage = sign_settings(&key);
    let request = rooftop::Request {
        lot_key: key.clone(),
        tenement: true,
        wall_tint,
        width,
        depth,
        parapet,
        ceiling,
        perimeter,
    };
    draw(cancelled, |mesh| {
        let proposals = rooftop::draw(
            &mut on(mesh, room_pen("rooftop", [-width / 2., -depth / 2., 0.])?)?,
            &request,
            &rooftop::Settings {
                kit: &kit,
                wall: binding("wall"),
                concrete: binding("concrete"),
                brushed: binding("brushed-metal"),
                plants: textured,
                cloth: textured,
            },
        )?;
        for proposal in &proposals {
            proposal.draw(mesh, &|_| 1., &signage)?;
        }
        Ok(())
    })
}

/// `kowloon/washing`: a seeded row of laundry on a line or pole, with the line or pole
/// itself. Anchor: the centre of the line on the wall plane; the line runs `offset` in
/// front of the wall at z = 0 and the washing hangs below it. Parameters: `support` (`line`
/// | `pole`, `line`), `length` (m, 3), `drop` (m of room below the line, 1.5), `fill`
/// (0–1 share of the line used, 1), `offset` (m, 0.3), `textured` (cloth atlas, true),
/// `seed`.
fn washing(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let support: laundry::Support = a.choice("support", "line", &["line", "pole"])?;
    let length = a.number("length", 3.)?;
    let options = laundry::WashingOptions {
        fill: a.number("fill", 1.)?,
        drop: a.number("drop", 1.5)?,
        support,
    };
    let offset = a.number("offset", 0.3)?;
    let textured = a.boolean("textured", true)?;
    let mut rng = a.rng()?;
    a.finish()?;
    let span = [-length / 2., length / 2.];
    draw(cancelled, |mesh| {
        let d = &mut on(mesh, pen("washing")?.moved([0., offset, 0.])?)?;
        laundry::washing(d, span, 0., &mut rng, &options, textured)?;
        d.tube(
            "paint",
            &[[span[0], 0., 0.], [span[1], 0., 0.]],
            if support == laundry::Support::Line {
                0.006
            } else {
                0.016
            },
            [0.4; 3],
            8,
        )
    })
}

/// `kowloon/hangers`: a seeded row of clothes hangers on a sloping rail, with the rail.
/// Anchor: on the wall plane below the rail's centre, at the height of its low (left) end;
/// the rail runs `offset` in front of the wall from z = 0 rising by `slope` per meter, and
/// the garments hang across it. Parameters: `length` (m, 3), `slope` (0.03), `reach` (m of
/// half-width room per garment, 0.6), `drop` (m, 1.5), `offset` (m, 0.5), `textured`
/// (true), `seed`.
fn hangers(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let length = a.number("length", 3.)?;
    let row = laundry::HangerRow {
        span: [-length / 2., length / 2.],
        z: 0.,
        slope: a.number("slope", 0.03)?,
        reach: a.number("reach", 0.6)?,
        drop: a.number("drop", 1.5)?,
    };
    let offset = a.number("offset", 0.5)?;
    let textured = a.boolean("textured", true)?;
    let mut rng = a.rng()?;
    a.finish()?;
    draw(cancelled, |mesh| {
        let d = &mut on(mesh, pen("hangers")?.moved([0., offset, 0.])?)?;
        laundry::hangers(d, &row, &mut rng, textured)?;
        d.tube(
            "paint",
            &[[row.span[0], 0., 0.], [row.span[1], 0., row.slope * length]],
            0.016,
            [0.4; 3],
            8,
        )
    })
}

/// The Kowloon sign settings: unweathered vertex-colored artwork with atlas lettering.
fn sign_settings(key: &str) -> signs::panel::Settings {
    signs::panel::Settings {
        weathering: weathering(key, 0.),
        timber: timber::Settings {
            textured: false,
            ..Default::default()
        },
        lettering: true,
    }
}

/// `kowloon/sign`: a flat shop sign panel with lettered text blocks. Anchor: bottom-centre
/// on the wall plane; the panel stands `depth` proud. Parameters: `width` (m, 2.4),
/// `height` (m, 0.9), `depth` (m, 0.08), `construction` (`sheet` | `lightbox` | `timber` |
/// `enamel` | `tile` | `cloth` | `painted`, `sheet`), `outline` (`rectangle` | `diamond` |
/// `round`, `rectangle`), `ground` (#0d4a3a), `border` (#d9b380), `bordered` (true),
/// `text` (main block; defaults to a seeded business name, "" omits it), `subtitle`
/// (second block; defaults to the seeded English name, "" omits it), `face` (`hei` |
/// `song` | `kai` | `brush` | `heavy` | `round` | `slim`, `kai`), `direction` (`ltr` |
/// `rtl` | `ttb`, `rtl`), `stack` (`down` | `across`, `down`), `ink` (#f0dca0), `shadow`
/// (true), `sides` (`front` | `both`, `front`), `age` (0–1, 0.3), `seed` (copy, phase).
fn sign(name: &str, values: &Values, cancelled: &dyn Fn() -> bool) -> Result<NativeOutput, String> {
    use signs::{lettering::Face, panel};
    let mut a = Arguments::new(name, values);
    let width = a.number("width", 2.4)?;
    let height = a.number("height", 0.9)?;
    let depth = a.number("depth", 0.08)?;
    let construction = a.choice(
        "construction",
        "sheet",
        &[
            "sheet", "lightbox", "timber", "enamel", "tile", "cloth", "painted",
        ],
    )?;
    let outline = a.choice("outline", "rectangle", &["rectangle", "diamond", "round"])?;
    let ground_color = a.color("ground", "#0d4a3a")?;
    let border = a.color("border", "#d9b380")?;
    let bordered = a.boolean("bordered", true)?;
    let text = a.optional_text("text")?;
    let subtitle = a.optional_text("subtitle")?;
    let face = a.choice(
        "face",
        "kai",
        &["hei", "song", "kai", "brush", "heavy", "round", "slim"],
    )?;
    let direction = a.choice("direction", "rtl", &["ltr", "rtl", "ttb"])?;
    let stack = a.choice("stack", "down", &["down", "across"])?;
    let ink = a.color("ink", "#f0dca0")?;
    let shadow = a.boolean("shadow", true)?;
    let sides = a.choice("sides", "front", &["front", "both"])?;
    let age = a.number("age", 0.3)?;
    let key = a.seed()?;
    a.finish()?;
    let mut rng = SeededRng::from_text(&key);
    let business = signs::copy::invent(&mut rng, &Default::default())?;
    let mut blocks = vec![];
    let main = text.unwrap_or(business.name);
    if !main.is_empty() {
        blocks.push(panel::Block {
            text: main,
            face,
            direction,
            ink,
            share: 2.,
            shadow: shadow.then_some([0.04, 0.08, 0.1]),
        });
    }
    let second = subtitle.unwrap_or(business.english);
    if !second.is_empty() {
        blocks.push(panel::Block {
            text: second,
            face: Face::Hei,
            direction: signs::lettering::Direction::Ltr,
            ink,
            share: 1.,
            shadow: None,
        });
    }
    let design = panel::Design {
        width,
        height,
        depth,
        construction,
        outline,
        ground: ground_color,
        border: bordered.then_some(border),
        stack,
        blocks,
        age,
        phase: rng.float(0., 100.),
    };
    let settings = sign_settings(&key);
    draw(cancelled, |mesh| {
        panel::draw(
            // As Kowloon's flat signs place them: the panel's face is its local y = 0 and
            // its body lies behind it, so the frame stands proud of the wall.
            &mut on(mesh, pen("sign")?.moved([-width / 2., 0.01 + depth, 0.])?)?,
            &design,
            sides,
            &panel::Hardware {
                slot: "metal".into(),
                tint: [0.25, 0.3, 0.28],
                weathering: None,
            },
            &settings,
        )
    })
}

/// A projecting sign mount: `blade_sign` or `hung_sign`.
type Mounting = fn(
    &signs::mounts::Site,
    signs::mounts::PierRoom,
    &signs::copy::Business,
    &signs::panel::Settings,
    &mut SeededRng,
) -> Result<Option<signs::mounts::Projected>, String>;

/// Shared body of the projecting sign generators.
fn projecting(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
    make: Mounting,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let ceiling = a.number("height", 4.6)?;
    let reach = a.number("reach", 1.5)?;
    let room = signs::mounts::PierRoom {
        u: 0.,
        foot: 0.,
        ceiling,
        reach,
    };
    let key = a.seed()?;
    a.finish()?;
    let settings = sign_settings(&key);
    let business = signs::copy::invent(
        &mut SeededRng::from_text(&format!("{key}/business")),
        &Default::default(),
    )?;
    let site = signs::mounts::Site {
        pen: pen("sign")?,
        elevation: 0.,
        key: key.clone(),
        shown: true,
        detail: 1,
        wash: None,
    };
    let projected = make(
        &site,
        room,
        &business,
        &settings,
        &mut SeededRng::from_text(&key),
    )?
    .ok_or_else(|| format!("{name}: no sign fits a {ceiling} m pier with {reach} m reach."))?;
    draw(cancelled, |mesh| {
        projected.proposal.draw(mesh, &|_| 1., &settings)
    })
}

/// `kowloon/blade-sign`: a projecting double-sided blade sign on its outriggers, with
/// seeded business copy and construction. Anchor: the pier line (x = 0) at the foot of the
/// mounting room on the wall plane. Parameters: `height` (m of pier room above the anchor,
/// 4.6), `reach` (m of clear projection, 1.5), `seed`.
fn blade_sign(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    projecting(name, values, cancelled, signs::mounts::blade_sign)
}

/// `kowloon/hung-sign`: a projecting sign hung below a cantilevered arm, with seeded
/// business copy. Anchor and parameters as `kowloon/blade-sign`.
fn hung_sign(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    projecting(name, values, cancelled, signs::mounts::hung_sign)
}

const GROCER_PROPS: [&str; 17] = [
    "rice-sack",
    "egg-basket",
    "garlic-braid",
    "counter-scale",
    "bottle-crate",
    "drink-bottle",
    "food-tin",
    "tea-carton",
    "tissue-roll",
    "shop-stool",
    "shop-counter",
    "drinks-cooler",
    "seasoning-jar",
    "dry-goods-bag",
    "bottle-load",
    "produce-crate",
    "stock-box",
];

/// `kowloon/grocer-prop`: one shipped grocer stock or equipment model fitted to a requested
/// size (room frame). Anchor: the model's own grocer anchor at the origin (its footprint
/// rests on z = 0). Parameters: `model` (one of rice-sack, egg-basket, garlic-braid,
/// counter-scale, bottle-crate, drink-bottle, food-tin, tea-carton, tissue-roll, shop-stool,
/// shop-counter, drinks-cooler, seasoning-jar, dry-goods-bag, bottle-load, produce-crate,
/// stock-box; `rice-sack`), `width` (m, 0.75), `depth` (m, 0.65), `height` (m, 0.8),
/// `yaw` (radians, 0), `variant` (label variant, 1–4, 1), `tint` (#ffffff), `textured`
/// (grocer atlases, true).
fn grocer_prop(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let model = a.text("model", "rice-sack")?;
    if !GROCER_PROPS.contains(&model.as_str()) {
        return Err(format!(
            "{name}: model must be one of {}.",
            GROCER_PROPS.join(", ")
        ));
    }
    let size = [
        a.number("width", 0.75)?,
        a.number("depth", 0.65)?,
        a.number("height", 0.8)?,
    ];
    let yaw = a.number("yaw", 0.)?;
    let options = shop::grocer_props::Options {
        variant: a.integer("variant", 1, 1..=4)?,
        tint: a.color("tint", "#ffffff")?,
        textured: a.boolean("textured", true)?,
        ..Default::default()
    };
    a.finish()?;
    draw(cancelled, |mesh| {
        shop::grocer_props::draw(
            &mut on(mesh, room_pen(&model, [0.; 3])?)?,
            &model,
            [0.; 3],
            size,
            yaw,
            &options,
        )
        .map(|_| ())
    })
}

/// `kowloon/shelving`: freestanding shop shelving with bays and decks (room frame).
/// Anchor: bottom-centre of its back on the wall plane; it occupies x in [-width/2,
/// width/2], y in [-depth, 0] and opens toward -Y. Parameters: `width` (m, 2), `depth` (m,
/// 0.35), `height` (m, 1.9), `levels` (1–64, 4), `material` (`timber` | `metal`,
/// `timber`), `color` (#8e6747), `above` (m of clear stock room above the top deck, 0.35).
fn shelving(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let size = [
        a.number("width", 2.)?,
        a.number("depth", 0.35)?,
        a.number("height", 1.9)?,
    ];
    let levels = a.integer("levels", 4, 1..=64)?;
    let slot = a.text("material", "timber")?;
    if slot != "timber" && slot != "metal" {
        return Err(format!("{name}: material must be one of timber, metal."));
    }
    let tint = a.color("color", "#8e6747")?;
    let above = a.number("above", 0.35)?;
    a.finish()?;
    draw(cancelled, |mesh| {
        shop::furniture::shelving(
            &mut on(mesh, room_pen("shelving", [-size[0] / 2., -size[1], 0.])?)?,
            size,
            levels,
            &slot,
            tint,
            above,
            &timber::Settings::default(),
        )
        .map(|_| ())
    })
}

/// `kowloon/counter`: a shop counter in timber, tile or steel (room frame). Anchor:
/// bottom-centre of its back; it occupies x in [-width/2, width/2], y in [-depth, 0].
/// Parameters: `width` (m, 1.4), `depth` (m, 0.5), `height` (m, 0.9), `finish` (`timber` |
/// `tiled` | `steel`, `timber`), `color` (#7a6851), `seed`.
fn counter(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let size = [
        a.number("width", 1.4)?,
        a.number("depth", 0.5)?,
        a.number("height", 0.9)?,
    ];
    let finish = a.choice("finish", "timber", &["timber", "tiled", "steel"])?;
    let tint = a.color("color", "#7a6851")?;
    let mut rng = a.rng()?;
    a.finish()?;
    draw(cancelled, |mesh| {
        shop::furniture::counter(
            &mut on(mesh, room_pen("counter", [-size[0] / 2., -size[1], 0.])?)?,
            size,
            finish,
            tint,
            &mut rng,
            &timber::Settings::default(),
        )
        .map(|_| ())
    })
}

/// `kowloon/crate`: a slatted timber stock crate (room frame). Anchor: bottom-centre of its
/// footprint. Parameters: `width` (m, 0.5), `depth` (m, 0.4), `height` (m, 0.3), `color`
/// (#a68454). The slat shades the crate draws from its stream are covered by the shipped
/// timber atlas, so it reads no seed.
fn crate_box(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let size = [
        a.number("width", 0.5)?,
        a.number("depth", 0.4)?,
        a.number("height", 0.3)?,
    ];
    let tint = a.color("color", "#a68454")?;
    a.finish()?;
    let mut rng = SeededRng::from_text(name);
    draw(cancelled, |mesh| {
        shop::furniture::crate_box(
            &mut on(mesh, room_pen("crate", [-size[0] / 2., -size[1] / 2., 0.])?)?,
            size,
            tint,
            &mut rng,
            &timber::Settings::default(),
        )
        .map(|_| ())
    })
}

/// `kowloon/bottle-crate`: a plastic soft-drink crate, optionally loaded with bottles
/// (room frame). Anchor: bottom-centre of its 0.42 × 0.3 m footprint.
/// Parameters: `color` (#a42c29), `glass` (#3a6a3c), `cap` (#cfa343), `loaded` (true).
fn bottle_crate(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let tint = a.color("color", "#a42c29")?;
    let glass = a.color("glass", "#3a6a3c")?;
    let cap = a.color("cap", "#cfa343")?;
    let loaded = a.boolean("loaded", true)?;
    a.finish()?;
    draw(cancelled, |mesh| {
        shop::fittings::bottle_crate(
            &mut on(mesh, room_pen("bottle-crate", [-0.21, -0.15, 0.])?)?,
            tint,
            glass,
            cap,
            loaded,
        )
    })
}

/// `kowloon/counter-scale`: a counter weighing scale (room frame). Anchor: bottom-centre.
/// Parameters: `color` (#759b8a).
fn counter_scale(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let tint = a.color("color", "#759b8a")?;
    a.finish()?;
    draw(cancelled, |mesh| {
        shop::fittings::counter_scale(
            &mut on(mesh, room_pen("counter-scale", [0.; 3])?)?,
            [0.; 3],
            tint,
        )
    })
}

/// `kowloon/price-card`: a seeded price card on a stick (room frame). Anchor: the foot of
/// the stick. Parameters: `seed`.
fn price_card(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let mut rng = a.rng()?;
    a.finish()?;
    draw(cancelled, |mesh| {
        shop::fittings::price_card(
            &mut on(mesh, room_pen("price-card", [0.; 3])?)?,
            [0.; 3],
            &mut rng,
        )
    })
}

/// `kowloon/pendant-lamp`: a ceiling pendant lamp (room frame). Anchor: the ceiling rose;
/// the lamp hangs below z = 0. Parameters: `drop` (m, 0.08–20, 0.5), `shaded` (true),
/// `color` (shade, #487b64).
fn pendant_lamp(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let drop = a.number("drop", 0.5)?;
    let shaded = a.boolean("shaded", true)?;
    let color = a.color("color", "#487b64")?;
    a.finish()?;
    draw(cancelled, |mesh| {
        shop::fittings::pendant(
            &mut on(mesh, room_pen("pendant-lamp", [0.; 3])?)?,
            [0.; 3],
            drop,
            shaded.then_some(color),
        )
    })
}

/// `kowloon/ceiling-fan`: a seeded ceiling fan on its downrod (room frame). Anchor: the
/// ceiling mount; the fan hangs below z = 0. Parameters: `drop` (m, 0.12–10, 0.28), `span`
/// (m blade diameter, 0.4–10, 0.9), `seed`.
fn ceiling_fan(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let drop = a.number("drop", 0.28)?;
    let span = a.number("span", 0.9)?;
    let mut rng = a.rng()?;
    a.finish()?;
    draw(cancelled, |mesh| {
        shop::fittings::ceiling_fan(
            &mut on(mesh, room_pen("ceiling-fan", [0.; 3])?)?,
            [0.; 3],
            drop,
            span,
            &mut rng,
        )
    })
}

/// `kowloon/tube-light`: a fluorescent batten (room frame). Anchor: the centre of the
/// batten's mounting face; it hangs below z = 0 along X. Parameters: `length` (m, ≥ 0.1,
/// 1.6).
fn tube_light(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let length = a.number("length", 1.6)?;
    a.finish()?;
    draw(cancelled, |mesh| {
        shop::fittings::tube_light(
            &mut on(mesh, room_pen("tube-light", [0.; 3])?)?,
            [-length / 2., 0., 0.],
            [length / 2., 0., 0.],
        )
    })
}

/// `kowloon/wall-clock`: a seeded wall clock (room frame). Anchor: the centre of the dial
/// on the wall plane; it faces -Y. Parameters: `seed`.
fn wall_clock(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let mut rng = a.rng()?;
    a.finish()?;
    draw(cancelled, |mesh| {
        shop::fittings::wall_clock(
            &mut on(mesh, room_pen("wall-clock", [0.; 3])?)?,
            [0.; 3],
            &mut rng,
        )
    })
}

/// `kowloon/calendar`: a seeded hanging wall calendar from the posters atlas (room frame).
/// Anchor: its hanging point on the wall plane; it faces -Y. Parameters: `seed`.
fn calendar(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let mut rng = a.rng()?;
    a.finish()?;
    draw(cancelled, |mesh| {
        shop::fittings::calendar(
            &mut on(mesh, room_pen("calendar", [0.; 3])?)?,
            [0.; 3],
            &mut rng,
            true,
        )
    })
}

/// `kowloon/hanging-scale`: a hanging dial scale with chains and pan (room frame). Anchor:
/// the hook point; it hangs below z = 0. Parameters: `drop` (m, 0.5–20, 0.7), `color`
/// (#4a7899).
fn hanging_scale(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let drop = a.number("drop", 0.7)?;
    let tint = a.color("color", "#4a7899")?;
    a.finish()?;
    draw(cancelled, |mesh| {
        shop::fittings::hanging_scale(
            &mut on(mesh, room_pen("hanging-scale", [0.; 3])?)?,
            [0.; 3],
            drop,
            tint,
        )
    })
}

/// `kowloon/stool`: a turned timber shop stool (room frame). Anchor: bottom-centre.
/// Parameters: `color` (#8e6747).
fn stool(
    name: &str,
    values: &Values,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeOutput, String> {
    let mut a = Arguments::new(name, values);
    let tint = a.color("color", "#8e6747")?;
    a.finish()?;
    draw(cancelled, |mesh| {
        shop::fittings::stool(&mut on(mesh, room_pen("stool", [0.; 3])?)?, [0.; 3], tint)
    })
}

/// Splits a render mesh into one ItemSpec mesh per material slot, in first-use order.
/// Vertex tints become vertex colors.
pub fn meshes(mesh: &Mesh<'_>) -> Result<Vec<ItemMesh>, String> {
    let mut order: Vec<String> = vec![];
    let mut groups: BTreeMap<String, (ItemMesh, BTreeMap<u32, u32>)> = BTreeMap::new();
    for (triangle, corners) in mesh.indices.chunks_exact(3).enumerate() {
        let surface = mesh
            .surfaces
            .get(
                *mesh
                    .triangle_surfaces
                    .get(triangle)
                    .ok_or("Unassigned triangle.")?,
            )
            .ok_or("Unknown surface.")?;
        let slot = surface.slot.clone();
        let (target, remap) = groups.entry(slot.clone()).or_insert_with(|| {
            order.push(slot.clone());
            (
                ItemMesh {
                    id: slot.clone(),
                    positions: vec![],
                    normals: vec![],
                    uvs: vec![],
                    additional_uvs: BTreeMap::new(),
                    colors: vec![],
                    tangents: vec![],
                    indices: vec![],
                    material_slot: slot.clone(),
                },
                BTreeMap::new(),
            )
        });
        for &corner in corners {
            let index = *remap.entry(corner).or_insert_with(|| {
                let vertex = mesh.vertices[corner as usize];
                target.positions.push(vertex.position);
                target.normals.push(vertex.normal);
                target.uvs.push(vertex.uv);
                target
                    .colors
                    .push([vertex.color[0], vertex.color[1], vertex.color[2], 1.]);
                target.positions.len() as u32 - 1
            });
            target.indices.push(index);
        }
    }
    Ok(order
        .into_iter()
        .filter_map(|slot| groups.remove(&slot).map(|(mesh, _)| mesh))
        .collect())
}

impl Natives {
    pub fn new() -> Self {
        let registered: &[(&'static str, Generate)] = &[
            ("kowloon/aircon", aircon),
            ("kowloon/condenser", condenser),
            ("kowloon/window", window),
            ("kowloon/security-bars", security_bars),
            ("kowloon/wall-vent", wall_vent),
            ("kowloon/roof-outlet", roof_outlet),
            ("kowloon/riser-duct", riser_duct),
            ("kowloon/downpipe", downpipe),
            ("kowloon/pipe-elbow", pipe_elbow),
            ("kowloon/balcony", balcony),
            ("kowloon/anti-climb", anti_climb),
            ("kowloon/plant", plant),
            ("kowloon/plants", plants),
            ("kowloon/cage", cage),
            ("kowloon/drying-rack", drying_rack),
            ("kowloon/drying-pole", drying_pole),
            ("kowloon/drying-lines", drying_lines),
            ("kowloon/awning", awning),
            ("kowloon/tarp", tarp),
            ("kowloon/board-sign", board_sign),
            ("kowloon/wall-lamp", wall_lamp),
            ("kowloon/meter-bank", meter_bank),
            ("kowloon/roller-shutter", roller_shutter),
            ("kowloon/entrance", entrance),
            ("kowloon/ground-bay", ground_bay),
            ("kowloon/water-tank", water_tank),
            ("kowloon/stair-hut", stair_hut),
            ("kowloon/aerial", aerial),
            ("kowloon/rooftop-shack", rooftop_shack),
            ("kowloon/rooftop", rooftop_deck),
            ("kowloon/washing", washing),
            ("kowloon/hangers", hangers),
            ("kowloon/sign", sign),
            ("kowloon/blade-sign", blade_sign),
            ("kowloon/hung-sign", hung_sign),
            ("kowloon/grocer-prop", grocer_prop),
            ("kowloon/shelving", shelving),
            ("kowloon/counter", counter),
            ("kowloon/crate", crate_box),
            ("kowloon/bottle-crate", bottle_crate),
            ("kowloon/counter-scale", counter_scale),
            ("kowloon/price-card", price_card),
            ("kowloon/pendant-lamp", pendant_lamp),
            ("kowloon/ceiling-fan", ceiling_fan),
            ("kowloon/tube-light", tube_light),
            ("kowloon/wall-clock", wall_clock),
            ("kowloon/calendar", calendar),
            ("kowloon/hanging-scale", hanging_scale),
            ("kowloon/stool", stool),
        ];
        Self {
            generators: registered.iter().copied().collect(),
        }
    }
}
impl Default for Natives {
    fn default() -> Self {
        Self::new()
    }
}
impl NativeGenerators for Natives {
    fn identity(&self, name: &str) -> Option<String> {
        self.generators
            .contains_key(name)
            .then(|| format!("{name}@{VERSION}"))
    }
    fn names(&self) -> Vec<String> {
        self.generators
            .keys()
            .map(|name| (*name).to_owned())
            .collect()
    }
    fn generate(
        &self,
        name: &str,
        parameters: &Values,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<NativeOutput, String> {
        let generate = self
            .generators
            .get(name)
            .ok_or_else(|| format!("Unknown native generator {name}."))?;
        generate(name, parameters, cancelled)
    }
}

/// The shared registry handed to ItemSpec hosts.
pub fn natives() -> Arc<dyn NativeGenerators> {
    Arc::new(Natives::new())
}
