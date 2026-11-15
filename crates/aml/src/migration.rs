//! Explicit mechanical conversion of historical SVGX XML to current AML.
use crate::format::escape;
use roxmltree::{Document, Node, ParsingOptions};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
};

fn number(n: Node<'_, '_>, name: &str, default: f64) -> Result<f64, String> {
    n.attribute(name)
        .map(|s| {
            s.parse::<f64>()
                .ok()
                .filter(|v| v.is_finite() && v.abs() <= 1e7)
                .ok_or_else(|| {
                    format!(
                        "Invalid {name} on {}.",
                        n.attribute("id").unwrap_or(n.tag_name().name())
                    )
                })
        })
        .unwrap_or(Ok(default))
}
fn required<'a>(n: Node<'a, 'a>, name: &str) -> Result<&'a str, String> {
    n.attribute(name)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("Missing {name} on {}.", n.tag_name().name()))
}
fn vector(n: Node<'_, '_>, name: &str, default: [f64; 3]) -> Result<[f64; 3], String> {
    let Some(value) = n.attribute(name) else {
        return Ok(default);
    };
    let values = value
        .split_whitespace()
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| format!("Invalid {name}."))?;
    values
        .try_into()
        .map_err(|_| format!("{name} requires three coordinates."))
}
fn position(p: [f64; 3]) -> String {
    format!("{} {} {}", p[0], p[1], p[2])
}
fn attrs(n: Node<'_, '_>, omit: &[&str]) -> String {
    n.attributes()
        .filter(|a| !omit.contains(&a.name()))
        .map(|a| format!(" {}=\"{}\"", a.name(), escape(a.value())))
        .collect()
}
fn path_refs(value: &str, paths: &BTreeMap<String, String>) -> Result<String, String> {
    value
        .split_whitespace()
        .map(|reference| {
            let reverse = reference.starts_with('-');
            let id = reference.trim_start_matches('-');
            paths
                .get(id)
                .map(|path| format!("{}{path}", if reverse { "-" } else { "" }))
                .ok_or_else(|| format!("Unknown boundary edge {id}."))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|p| p.join(" "))
}
fn allowed(tag: &str) -> Option<&'static str> {
    Some(match tag {
        "level" => {
            "version id name validation description units wall-thickness wall-height floor-thickness curve-tolerance expect-fail"
        }
        "layer" => "id label",
        "vertex" => "id label x y z",
        "edge" => {
            "id label from to kind control height thickness material join base-floor top-floor span-floors"
        }
        "region" => {
            "id label boundary fill interior material ceiling thickness role lower upper rise underside area"
        }
        "opening" => "id label kind start end sill head barricade",
        "hole" => "boundary",
        "crease" => "from to",
        "seam" => "id label edges kind material",
        "material" => "id label color repeat roughness metalness texture",
        "marker" => "id label layer region position kind team movement",
        "route" => "id label from to min-routes min-distance max-distance movement",
        "link" => "id label from to kind bidirectional ability movement from-area to-area",
        "light" => "id label kind position target color intensity angle",
        "cover" => "id label layer min max material dynamic role",
        "asset" => "id label src",
        "prop" => "id label layer shape asset position rotation scale material collision segments",
        "reference" => {
            "id label image width height position target up fov near far scale scale-note"
        }
        "landmark" => "id label position image",
        "mask" => "id label points",
        "player" => "radius height step slope crouch vault rappel ladder",
        "boat" => "radius draft height",
        "sky" => "color intensity",
        "no-spawn-los" => "markers",
        "review-suppression" => "suggestion fingerprint reason",
        _ => return None,
    })
}

pub struct ImportedGeometry {
    pub points: Vec<[f64; 3]>,
    pub triangles: Vec<[usize; 3]>,
}

pub fn migrate(
    source: &str,
    cancelled: &dyn Fn() -> bool,
    import: &dyn Fn(&str) -> Result<ImportedGeometry, String>,
) -> Result<String, String> {
    if source.len() > 40_000_000 {
        return Err("Migration source exceeds its byte allowance.".into());
    }
    let xml = Document::parse_with_options(
        source,
        ParsingOptions {
            allow_dtd: false,
            nodes_limit: 1000000,
            ..Default::default()
        },
    )
    .map_err(|e| e.to_string())?;
    let root = xml.root_element();
    if root.tag_name().name() != "level" || root.attribute("version") != Some("2") {
        return Err("Migration requires historical <level version=\"2\"> XML.".into());
    }
    if root.attribute("units").is_some_and(|u| u != "meters") {
        return Err("Migration source units must be meters.".into());
    }
    let mut ids = BTreeSet::new();
    for n in root.descendants().filter(Node::is_element) {
        if cancelled() {
            return Err("Migration cancelled.".into());
        }
        if n.ancestors().count() > 128 {
            return Err("Migration source nesting exceeds its allowance.".into());
        }
        let known = allowed(n.tag_name().name())
            .ok_or_else(|| format!("Unknown historical element {}.", n.tag_name().name()))?;
        if let Some(parent) = n.parent_element() {
            let valid = match parent.tag_name().name() {
                "level" => ![
                    "vertex", "edge", "region", "opening", "hole", "crease", "landmark", "mask",
                    "level",
                ]
                .contains(&n.tag_name().name()),
                "layer" => ["vertex", "edge", "region"].contains(&n.tag_name().name()),
                "edge" => n.has_tag_name("opening"),
                "region" => ["hole", "crease"].contains(&n.tag_name().name()),
                "reference" => ["landmark", "mask"].contains(&n.tag_name().name()),
                _ => false,
            };
            if !valid {
                return Err(format!(
                    "Invalid historical child {} inside {}.",
                    n.tag_name().name(),
                    parent.tag_name().name()
                ));
            }
        }
        for a in n.attributes() {
            if !known.split_whitespace().any(|name| name == a.name()) {
                return Err(format!(
                    "Unknown historical attribute {} on {}.",
                    a.name(),
                    n.tag_name().name()
                ));
            }
        }
        if let Some(id) = n.attribute("id") {
            if !ids.insert(id.to_owned()) {
                return Err(format!("Duplicate historical id {id}."));
            }
        }
        if n.children()
            .any(|c| c.is_text() && !c.text().unwrap_or("").trim().is_empty())
        {
            return Err("Historical XML uses attributes for text.".into());
        }
    }
    let mut unique = |base: String| {
        let mut id = base.clone();
        let mut i = 0;
        while ids.contains(&id) {
            i += 1;
            id = format!("{base}:{i}");
        }
        ids.insert(id.clone());
        id
    };
    let mut out = format!(
        "<architecture version=\"1\"{}>\n",
        attrs(
            root,
            &[
                "version",
                "units",
                "wall-thickness",
                "wall-height",
                "floor-thickness",
                "expect-fail"
            ]
        )
    );
    let wall_height = number(root, "wall-height", 3.)?;
    let thickness = number(root, "wall-thickness", 0.22)?;
    let floor_thickness = number(root, "floor-thickness", 0.2)?;
    let mut paths = BTreeMap::new();
    let mut endpoints = BTreeMap::new();
    let mut owners: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut region_kinds = BTreeMap::new();
    let mut vertices = BTreeMap::new();
    for n in root.descendants().filter(|n| n.has_tag_name("vertex")) {
        vertices.insert(
            required(n, "id")?.to_owned(),
            [
                number(n, "x", f64::NAN)?,
                number(n, "y", f64::NAN)?,
                number(n, "z", f64::NAN)?,
            ],
        );
    }
    for e in root.descendants().filter(|n| n.has_tag_name("edge")) {
        let id = required(e, "id")?;
        let path = unique(format!("{id}:path"));
        let a = *vertices
            .get(required(e, "from")?)
            .ok_or("Unknown edge start.")?;
        let b = *vertices
            .get(required(e, "to")?)
            .ok_or("Unknown edge end.")?;
        if let Some(control) = e.attribute("control") {
            let points = control
                .split(';')
                .map(|p| format!("{} 0", p.trim()))
                .collect::<Vec<_>>()
                .join("; ");
            writeln!(out,"<path id=\"{}\" points=\"{}\" elevation=\"{} {}\"><bezier controls=\"{}\" to=\"{}\"/></path>",escape(&path),position(a),a[2],b[2],escape(&points),position(b)).unwrap();
        } else {
            writeln!(
                out,
                "<path id=\"{}\" points=\"{}; {}\"/>",
                escape(&path),
                position(a),
                position(b)
            )
            .unwrap();
        }
        paths.insert(id.to_owned(), path);
        endpoints.insert(id.to_owned(), (a, b));
    }
    for r in root.descendants().filter(|n| n.has_tag_name("region")) {
        region_kinds.insert(required(r, "id")?, r.attribute("fill").unwrap_or("floor"));
        for edge in required(r, "boundary")?.split_whitespace() {
            owners
                .entry(edge.trim_start_matches('-').into())
                .or_default()
                .push(required(r, "id")?.into());
        }
    }
    let wall_group = unique("migration:static-walls".into());
    let mut static_walls = String::new();
    let mut openings = String::new();
    for layer in root.children().filter(|n| n.has_tag_name("layer")) {
        let group = required(layer, "id")?;
        let mut body = String::new();
        for r in layer.children().filter(|n| n.has_tag_name("region")) {
            let id = required(r, "id")?;
            let profile = unique(format!("{id}:profile"));
            let interior = r
                .attribute("interior")
                .unwrap_or("")
                .split_whitespace()
                .map(|v| {
                    vertices
                        .get(v)
                        .copied()
                        .map(position)
                        .ok_or_else(|| format!("Unknown interior vertex {v}."))
                })
                .collect::<Result<Vec<_>, _>>()?
                .join("; ");
            writeln!(
                out,
                "<profile id=\"{}\" paths=\"{}\" interior=\"{}\">",
                escape(&profile),
                escape(&path_refs(required(r, "boundary")?, &paths)?),
                interior
            )
            .unwrap();
            for h in r.children().filter(Node::is_element) {
                match h.tag_name().name() {
                    "hole" => writeln!(
                        out,
                        "<hole paths=\"{}\"/>",
                        escape(&path_refs(required(h, "boundary")?, &paths)?)
                    )
                    .unwrap(),
                    "crease" => {
                        let a = vertices
                            .get(required(h, "from")?)
                            .ok_or("Unknown crease start.")?;
                        let b = vertices
                            .get(required(h, "to")?)
                            .ok_or("Unknown crease end.")?;
                        writeln!(
                            out,
                            "<breakline points=\"{}; {}\"/>",
                            position(*a),
                            position(*b)
                        )
                        .unwrap();
                    }
                    _ => return Err("Unsupported region child.".into()),
                }
            }
            out.push_str("</profile>\n");
            let base = attrs(
                r,
                &[
                    "boundary",
                    "fill",
                    "interior",
                    "ceiling",
                    "thickness",
                    "role",
                    "lower",
                    "upper",
                    "rise",
                    "underside",
                ],
            );
            let fill = r.attribute("fill").unwrap_or("floor");
            if ["stairs", "ramp"].contains(&fill) {
                let refs = required(r, "boundary")?
                    .split_whitespace()
                    .collect::<Vec<_>>();
                let lo = refs
                    .iter()
                    .position(|s| s.trim_start_matches('-') == r.attribute("lower").unwrap_or(""))
                    .ok_or("Flight lower landing is outside its boundary.")?;
                let hi = refs
                    .iter()
                    .position(|s| s.trim_start_matches('-') == r.attribute("upper").unwrap_or(""))
                    .ok_or("Flight upper landing is outside its boundary.")?;
                let between = |start: usize, end: usize| {
                    let mut p = vec![];
                    let mut i = (start + 1) % refs.len();
                    while i != end {
                        p.push(refs[i]);
                        i = (i + 1) % refs.len();
                    }
                    p
                };
                let left = unique(format!("{id}:left"));
                let right = unique(format!("{id}:right"));
                let l = path_refs(&between(lo, hi).join(" "), &paths)?;
                let reversed = between(hi, lo)
                    .into_iter()
                    .rev()
                    .map(|s| {
                        if let Some(s) = s.strip_prefix('-') {
                            s.into()
                        } else {
                            format!("-{s}")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                writeln!(
                    out,
                    "<path id=\"{}\" paths=\"{}\"/><path id=\"{}\" paths=\"{}\"/>",
                    escape(&left),
                    escape(&l),
                    escape(&right),
                    escape(&path_refs(&reversed, &paths)?)
                )
                .unwrap();
                writeln!(
                    body,
                    "<{fill}{base} left=\"{}\" right=\"{}\" thickness=\"{}\" underside=\"{}\"{} />",
                    escape(&left),
                    escape(&right),
                    number(r, "thickness", floor_thickness)?,
                    escape(r.attribute("underside").unwrap_or("filled")),
                    if fill == "stairs" {
                        format!(" maximum-riser=\"{}\"", number(r, "rise", 0.18)?)
                    } else {
                        String::new()
                    }
                )
                .unwrap();
            } else if fill == "water" {
                writeln!(
                    body,
                    "<water{base} profile=\"{}\" elevation=\"0\"/>",
                    escape(&profile)
                )
                .unwrap();
            } else if fill == "floor" {
                writeln!(
                    body,
                    "<slab{base} profile=\"{}\" thickness=\"{}\"/>",
                    escape(&profile),
                    number(r, "thickness", floor_thickness)?
                )
                .unwrap();
            } else {
                return Err(format!("Unknown region fill {fill}."));
            }
            if r.attribute("ceiling").is_some() {
                let material = r
                    .attribute("material")
                    .map(|m| format!(" material=\"{}\"", escape(m)))
                    .unwrap_or_default();
                writeln!(body,"<roof id=\"{}\" profile=\"{}\" elevation=\"{}\" thickness=\"{floor_thickness}\" flat=\"true\"{material}/>",escape(&unique(format!("{id}:ceiling"))),escape(&profile),number(r,"ceiling",0.)?+floor_thickness).unwrap();
            }
        }
        for e in layer.children().filter(|n| n.has_tag_name("edge")) {
            let id = required(e, "id")?;
            let kind = e.attribute("kind").unwrap_or("wall");
            let path = &paths[id];
            let region_owners = owners.get(id).cloned().unwrap_or_default();
            if region_owners.len() == 2
                && (kind == "open"
                    || ["door", "arch", "breach", "rappel_door"].contains(&kind)
                    || e.children().any(|n| n.has_tag_name("opening")))
            {
                writeln!(
                    out,
                    "<connection id=\"{}\" from=\"{}\" to=\"{}\" path=\"{}\"/>",
                    escape(&unique(format!("{id}:connection"))),
                    escape(&region_owners[0]),
                    escape(&region_owners[1]),
                    escape(path)
                )
                .unwrap();
            }
            if kind == "open" {
                continue;
            }
            if ![
                "wall",
                "soft",
                "glass",
                "railing",
                "door",
                "window",
                "arch",
                "breach",
                "rappel_window",
                "rappel_door",
            ]
            .contains(&kind)
            {
                return Err(format!("Unknown edge kind {kind}."));
            }
            let dynamic = ["soft", "glass"].contains(&kind);
            let width = number(e, "thickness", thickness)?;
            let mut wall = format!(
                "<wall{} group=\"{}\" path=\"{}\" thickness=\"{width}\" height=\"{}\" dynamic=\"{dynamic}\"",
                attrs(e, &["from", "to", "kind", "control", "thickness", "height"]),
                escape(group),
                escape(path),
                number(
                    e,
                    "height",
                    if kind == "railing" { 1.1 } else { wall_height }
                )?
            );
            if let Some(floor) = e.attribute("base-floor").or_else(|| {
                (region_owners.len() == 1
                    && region_kinds.get(region_owners[0].as_str()) != Some(&"stairs"))
                .then(|| region_owners[0].as_str())
            }) {
                write!(wall, " fit-floor=\"{}\"", escape(floor)).unwrap();
            }
            wall.push_str("/>\n");
            if dynamic {
                body.push_str(&wall);
            } else {
                static_walls.push_str(&wall);
            }
            let mut cuts = e
                .children()
                .filter(|n| n.has_tag_name("opening"))
                .map(|n| {
                    Ok((
                        required(n, "id")?.to_owned(),
                        number(n, "start", 0.)?,
                        number(n, "end", 0.)?,
                        number(n, "sill", 0.)?,
                        number(n, "head", 2.1)?,
                        n.attribute("barricade") == Some("true"),
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            if [
                "door",
                "window",
                "arch",
                "breach",
                "rappel_window",
                "rappel_door",
            ]
            .contains(&kind)
            {
                let (a, b) = endpoints[id];
                let length = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
                cuts.push((
                    unique(format!("{id}:opening")),
                    0.,
                    length,
                    if kind.contains("window") { 0.9 } else { 0. },
                    if kind == "arch" { 2.4 } else { 2.1 },
                    false,
                ));
            }
            for (oid, start, end, sill, head, barricade) in cuts {
                let profile = unique(format!("{oid}:profile"));
                writeln!(out,"<profile id=\"{}\" points=\"{start} {sill}; {end} {sill}; {end} {head}; {start} {head}\"/>",escape(&profile)).unwrap();
                writeln!(openings,"<opening id=\"{}\" group=\"{}\" host=\"{}\" shape=\"profile\" profile=\"{}\" center=\"0 0\" width=\"{}\" height=\"{}\"/>",escape(&oid),escape(group),escape(id),escape(&profile),end-start,head-sill).unwrap();
                if barricade {
                    writeln!(body,"<panel id=\"{}\" opening=\"{}\" depth=\"{width}\" offset=\"{}\" border=\"0\" dynamic=\"true\"{}/>",escape(&unique(format!("{oid}:panel"))),escape(&oid),-width/2.,e.attribute("material").map(|m|format!(" material=\"{}\"",escape(m))).unwrap_or_default()).unwrap();
                }
            }
        }
        writeln!(out, "<group{}>{body}</group>", attrs(layer, &[])).unwrap();
    }
    for n in root.children().filter(Node::is_element) {
        let tag = n.tag_name().name();
        match tag {
            "layer" => {}
            "cover" => {
                let min = vector(n, "min", [0.; 3])?;
                let max = vector(n, "max", [0.; 3])?;
                writeln!(
                    out,
                    "<box{} group=\"{}\" position=\"{}\" width=\"{}\" depth=\"{}\" height=\"{}\"/>",
                    attrs(n, &["layer", "min", "max", "role"]),
                    escape(required(n, "layer")?),
                    position(min),
                    max[0] - min[0],
                    max[1] - min[1],
                    max[2] - min[2]
                )
                .unwrap();
            }
            "marker" => writeln!(
                out,
                "<marker{} group=\"{}\"{}/>",
                attrs(n, &["layer", "region"]),
                escape(required(n, "layer")?),
                n.attribute("region")
                    .map(|r| format!(" element=\"{}\"", escape(r)))
                    .unwrap_or_default()
            )
            .unwrap(),
            "reference" => {
                writeln!(out, "<reference{}>", attrs(n, &[])).unwrap();
                for c in n.children().filter(Node::is_element) {
                    writeln!(out, "<{}{} />", c.tag_name().name(), attrs(c, &[])).unwrap();
                }
                out.push_str("</reference>\n");
            }
            "seam" => {
                let pair = required(n, "edges")?.split_whitespace().collect::<Vec<_>>();
                if pair.len() != 2 {
                    return Err("A seam needs two edge IDs.".into());
                }
                let a = endpoints.get(pair[0]).ok_or("Unknown seam edge.")?;
                let b = endpoints.get(pair[1]).ok_or("Unknown seam edge.")?;
                let (lo, hi) = if a.0[2] <= b.0[2] {
                    (pair[0], pair[1])
                } else {
                    (pair[1], pair[0])
                };
                let kind = n.attribute("kind").unwrap_or("open");
                let low_owner = owners.get(lo).and_then(|v| v.first());
                let high_owner = owners.get(hi).and_then(|v| v.first());
                let floors = low_owner
                    .map(|id| format!(" lower-floor=\"{}\"", escape(id)))
                    .unwrap_or_default()
                    + &high_owner
                        .map(|id| format!(" upper-floor=\"{}\"", escape(id)))
                        .unwrap_or_default();
                writeln!(static_walls,"<retaining-wall{} lower=\"{}\" upper=\"{}\" thickness=\"{thickness}\" height=\"{wall_height}\" mode=\"{}\"{floors}/>",attrs(n,&["edges","kind"]),escape(&paths[lo]),escape(&paths[hi]),escape(kind)).unwrap();
                let step = root
                    .children()
                    .find(|n| n.has_tag_name("player"))
                    .map(|n| number(n, "step", 0.4))
                    .transpose()?
                    .unwrap_or(0.4);
                let reversed = (a.0[0] - b.0[0]).hypot(a.0[1] - b.0[1]) > 1e-7;
                let heights = if reversed {
                    [b.1[2], b.0[2]]
                } else {
                    [b.0[2], b.1[2]]
                };
                if kind == "open"
                    && (a.0[2] - heights[0]).abs().max((a.1[2] - heights[1]).abs()) <= step
                {
                    if let (Some(from), Some(to)) = (low_owner, high_owner) {
                        writeln!(
                            out,
                            "<connection id=\"{}\" from=\"{}\" to=\"{}\" path=\"{}\"/>",
                            escape(&unique(format!("{}:connection", required(n, "id")?))),
                            escape(from),
                            escape(to),
                            escape(&paths[lo])
                        )
                        .unwrap();
                    }
                }
            }
            "asset" => {}
            "prop" => migrate_prop(n, root, &mut out, &mut unique, import)?,
            _ => writeln!(out, "<{tag}{} />", attrs(n, &[])).unwrap(),
        }
    }
    writeln!(out,"<assembly id=\"{}\" coincident-material=\"retained-face\">{static_walls}</assembly>{openings}</architecture>",escape(&wall_group)).unwrap();
    crate::architecture::parse(&out).map_err(|e| e.to_string())?;
    Ok(out)
}

fn migrate_prop(
    n: Node<'_, '_>,
    root: Node<'_, '_>,
    out: &mut String,
    unique: &mut dyn FnMut(String) -> String,
    import: &dyn Fn(&str) -> Result<ImportedGeometry, String>,
) -> Result<(), String> {
    let id = required(n, "id")?;
    let group = unique(format!("{id}:placement"));
    writeln!(
        out,
        "<group id=\"{}\" group=\"{}\" position=\"{}\" rotation=\"{}\" scale=\"{}\">",
        escape(&group),
        escape(required(n, "layer")?),
        escape(required(n, "position")?),
        escape(n.attribute("rotation").unwrap_or("0 0 0")),
        escape(n.attribute("scale").unwrap_or("1 1 1"))
    )
    .unwrap();
    let collision = required(n, "collision")?;
    if !["solid", "none", "box"].contains(&collision) {
        return Err("Unknown prop collision mode.".into());
    }
    let material = n
        .attribute("material")
        .map(|m| format!(" material=\"{}\"", escape(m)))
        .unwrap_or_default();
    match n.attribute("shape").unwrap_or("box") {
        "box"=>writeln!(out,"<box id=\"{}\" anchor=\"bottom-center\" width=\"1\" depth=\"1\" height=\"1\" collidable=\"{}\"{material}/>",escape(id),collision!="none").unwrap(),
        "cylinder"=>{
            writeln!(out,"<cylinder id=\"{}\" radius=\"0.5\" height=\"1\" segments=\"{}\" collidable=\"{}\"{material}/>",escape(id),number(n,"segments",16.)?,collision=="solid").unwrap();
            if collision=="box"{writeln!(out,"<box id=\"{}\" anchor=\"bottom-center\" width=\"1\" depth=\"1\" height=\"1\" visible=\"false\"/>",escape(&unique(format!("{id}:collision")))).unwrap();}
        }
        "asset"=>{
            let asset=required(n,"asset")?;
            let definition=root.children().find(|a|a.has_tag_name("asset")&&a.attribute("id")==Some(asset)).ok_or("Prop references an unknown asset.")?;
            let src=required(definition,"src")?;
            writeln!(out,"<visual-mesh id=\"{}\" src=\"{}\" collidable=\"false\"{material}/>",escape(id),escape(src)).unwrap();
            if collision!="none"{
                let g=import(src)?;
                let collision_id=unique(format!("{id}:collision"));
                if collision=="solid" {
                    let points=g.points.into_iter().map(position).collect::<Vec<_>>().join("; ");
                    let triangles=g.triangles.iter().map(|t|format!("{} {} {}",t[0],t[1],t[2])).collect::<Vec<_>>().join("; ");
                    writeln!(out,"<faceted-solid id=\"{}\" points=\"{}\" triangles=\"{triangles}\" visible=\"false\"/>",escape(&collision_id),points).unwrap();
                }else{
                    let mut lo=[f64::INFINITY;3];let mut hi=[f64::NEG_INFINITY;3];
                    for p in g.points{for i in 0..3{lo[i]=lo[i].min(p[i]);hi[i]=hi[i].max(p[i]);}}
                    writeln!(out,"<box id=\"{}\" position=\"{}\" width=\"{}\" depth=\"{}\" height=\"{}\" visible=\"false\"/>",escape(&collision_id),position(lo),hi[0]-lo[0],hi[1]-lo[1],hi[2]-lo[2]).unwrap();
                }
            }
        }
        _=>return Err("Unknown historical prop shape.".into()),
    }
    out.push_str("</group>\n");
    Ok(())
}
