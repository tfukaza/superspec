//! Real pavement source images and Rust raster delivery, including holes and flush deck finishes.
use std::collections::BTreeMap;
use superspec_aml::{material::Material, material_catalog};
use superspec_engine::{
    Asset, Scene, compile,
    materialspec::surface::Inputs,
    worldspec::themes::kowloon::{
        mesh::{Mesh, Mode},
        pavement::{
            context::Wall,
            field, layout,
            plan::Settings,
            render::{self, Assets, Site},
            surface::{Finish, Source},
        },
        weathered,
    },
};
use superspec_geometry::image;
use superspec_geometry::obstructions::Budget;
pub fn scene() -> Scene {
    let mut scene=compile(r#"<architecture version="1" id="rust-pavement"><box id="host" width="11" depth="9" height=".1" visible="false"/></architecture>"#,0).unwrap();
    let mut materials = BTreeMap::new();
    let mut sources = BTreeMap::new();
    let looks = [
        (Finish::Concrete, "concrete", "#c4c1b5", 0.94, 0.),
        (Finish::Repair, "repair", "#bbb9af", 0.96, 0.),
        (Finish::Tile, "tile", "#58635e", 0.68, 0.),
        (Finish::Timber, "timber", "#ffffff", 0.8, 0.),
        (Finish::Grout, "grout", "#58584d", 1., 0.),
        (Finish::Substrate, "substrate", "#96917e", 0.98, 0.),
        (Finish::Asphalt, "asphalt", "#ffffff", 0.94, 0.),
        (Finish::ManholeRim, "manhole-rim", "#494940", 0.75, 0.65),
        (Finish::ManholeCover, "manhole-cover", "#68665b", 0.72, 0.7),
        (Finish::ManholeSlot, "manhole-slot", "#292b26", 0.95, 0.2),
        (Finish::Crack, "crack", "#36392e", 1., 0.),
        (Finish::Joint, "joint", "#626153", 1., 0.),
        (Finish::Spall, "spall", "#777465", 1., 0.),
        (Finish::Chip, "chip", "#b5b0a0", 0.98, 0.),
        (Finish::TileGrime, "tile-grime", "#725139", 1., 0.),
    ];
    for (finish, name, color, roughness, metalness) in looks {
        let base = match finish {
            Finish::Tile => "ceramic",
            Finish::Timber => "timber",
            Finish::Asphalt => "asphalt",
            _ => "concrete",
        };
        let mut material = if matches!(
            finish,
            Finish::TileGrime | Finish::Crack | Finish::ManholeRim | Finish::ManholeSlot
        ) {
            Material::default()
        } else {
            material_catalog::material(&format!("builtin:{base}")).unwrap()
        };
        material.id = format!("ground/{name}");
        material.color = color.into();
        material.roughness = roughness;
        material.metalness = metalness;
        sources.insert(finish, source(&material));
        materials.insert(material.id.clone(), material);
    }
    let mut bed = material_catalog::material("builtin:tile-bed").unwrap();
    bed.id = "ground/tile-bed".into();
    let tile_bed = source(&bed);
    materials.insert(bed.id.clone(), bed);
    let mut extra =
        |name: &str, color: &str, roughness: f64, metalness: f64, opacity: Option<f64>| {
            let m = Material {
                id: format!("ground/{name}"),
                color: color.into(),
                roughness,
                metalness,
                opacity,
                ..Material::default()
            };
            let s = source(&m);
            materials.insert(m.id.clone(), m);
            s
        };
    let assets = Assets {
        sources,
        tile_bed,
        edge: extra("edge", "#9c998c", 0.96, 0., None),
        water: extra("water-film", "#2f3936", 0.1, 0., Some(0.16)),
        paper: extra("paper", "#c9baa0", 0.96, 0., None),
        brick: extra("brick", "#796451", 0.98, 0., None),
        can: extra("can", "#a5a394", 0.4, 0.7, None),
    };
    let mut images = BTreeMap::new();
    for (name, bytes) in [
        (
            "concrete-shared.png",
            include_bytes!("../../../../assets/materials/concrete-shared.png").as_slice(),
        ),
        (
            "concrete-shared-normal.png",
            include_bytes!("../../../../assets/materials/concrete-shared-normal.png").as_slice(),
        ),
        (
            "ground-asphalt.png",
            include_bytes!("../../../../assets/materials/ground-asphalt.png").as_slice(),
        ),
        (
            "ground-asphalt-normal.png",
            include_bytes!("../../../../assets/materials/ground-asphalt-normal.png").as_slice(),
        ),
        (
            "timber.png",
            include_bytes!("../../../../assets/materials/timber.png").as_slice(),
        ),
        (
            "wall-ceramic-weathered.png",
            include_bytes!("../../../../assets/materials/wall-ceramic-weathered.png").as_slice(),
        ),
        (
            "wall-ceramic-normal.png",
            include_bytes!("../../../../assets/materials/wall-ceramic-normal.png").as_slice(),
        ),
    ] {
        let id = format!("superspec-material:{name}");
        images.insert(id.clone(), image::decode(bytes).unwrap());
        scene.manifest.assets.push(Asset {
            id,
            mime: "image/png",
            byte_start: scene.asset_bytes.len(),
            byte_length: bytes.len(),
        });
        scene.asset_bytes.extend_from_slice(bytes);
    }
    scene.manifest.asset_byte_count = scene.asset_bytes.len();
    scene.manifest.materials = materials.values().cloned().collect();
    let mut mesh = Mesh::new(Mode::Render, 200_000, &|| false).unwrap();
    let mut budget = Budget::new(64_000_000, &|| false).unwrap();
    for i in 0..4 {
        let x = (i % 2) as f64 * 6.;
        let y = (i / 2) as f64 * 5.;
        let z = 0.1;
        let outline = field::rectangle(x, y, 5., 4., z);
        let mut faces = Vec::new();
        let mut add = |px, py, w, d| {
            let p = field::rectangle(x + px, y + py, w, d, z);
            faces.push(vec![p[0], p[1], p[2]]);
            faces.push(vec![p[0], p[2], p[3]]);
        };
        if i == 1 {
            add(0., 0., 5., 1.5);
            add(0., 1.5, 1.5, 2.5);
            add(3., 1.5, 2., 2.5);
            add(1.5, 3., 1.5, 1.);
        } else {
            add(0., 0., 5., 4.);
        }
        let settings = Settings {
            patch_size: 2.4,
            material_size: 6.,
            tile_share: if i == 1 { 1. } else { 0. },
            timber_share: if i == 2 { 1. } else { 0. },
            asphalt: if i == 0 { 1. } else { 0. },
            manholes: 1.,
            litter: 1.,
            wetness: 0.7,
            breakup: 1.,
            ..Settings::default()
        };
        let layout = layout::footprint(
            &[outline],
            settings.patch_size,
            settings.material_size,
            &mut budget,
        )
        .unwrap();
        let walls = vec![
            Wall {
                host: "west".into(),
                seam: [[x + 0.1, y, z], [x + 0.1, y + 4., z]],
            },
            Wall {
                host: "divider".into(),
                seam: [[x + 1.8, y, z], [x + 1.8, y + 2.4, z]],
            },
        ];
        let seams = walls.iter().map(|w| w.seam).collect::<Vec<_>>();
        let host = format!("panel-{i}");
        let seed = format!("ground-render/{i}");
        let site = Site {
            host: &host,
            part: &host,
            seed: &seed,
            layout: &layout,
            faces: &faces,
            seams: &seams,
            walls: &walls,
            settings: &settings,
            grime: 0.6,
            wear: 0.7,
            sampling: 0.3,
            tile_size: 0.4,
            tile_aspect: None,
            tile_wear: None,
            tile_grime: None,
            density: Some(if i == 1 { 128. } else { 48. }),
            asphalt_density: None,
            fine_marks: true,
            flush: i == 3,
        };
        render::draw_preserved(&mut mesh, &site, &assets, &mut budget).unwrap();
    }
    let plan = weathered::finish(&mut mesh, &|| false).unwrap();
    let inputs = Inputs {
        materials: &materials,
        images: &images,
    };
    #[cfg(not(target_arch = "wasm32"))]
    let bakes = plan
        .bake_parallel(
            &inputs,
            "pavement",
            &weathered::ParallelBaking::default(),
            &|| false,
        )
        .unwrap();
    #[cfg(target_arch = "wasm32")]
    let bakes = plan.bake(&inputs, "pavement", &|| false).unwrap();
    let mut bindings: BTreeMap<_, _> = materials
        .keys()
        .map(|id| (id.clone(), id.clone()))
        .collect();
    bindings.extend(
        bakes
            .iter()
            .map(|b| (b.material().id.clone(), b.material().id.clone())),
    );
    scene
        .append_baked_visual("host", &mesh, &bindings, &bakes)
        .unwrap();
    scene
}
fn source(m: &Material) -> Source {
    Source {
        id: m.id.clone(),
        repeat: m.repeat,
        roughness: Some(m.roughness),
    }
}
