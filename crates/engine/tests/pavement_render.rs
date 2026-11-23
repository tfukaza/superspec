use serde_json::{Value, json};
use std::collections::BTreeMap;
use superspec_engine::worldspec::themes::kowloon::{
    mesh::{Mesh, Mode},
    pavement::{
        context::Wall,
        field, layout,
        plan::Settings,
        render::{self, Assets, Site},
        surface::{Finish, Source},
    },
    weathered,
};
use superspec_geometry::obstructions::Budget;
fn source(name: &str) -> Source {
    Source {
        id: format!("ground/{name}"),
        repeat: 1.3,
        roughness: Some(0.9),
    }
}
fn assets() -> Assets {
    let finishes = [
        (Finish::Concrete, "concrete"),
        (Finish::Repair, "repair"),
        (Finish::Tile, "tile"),
        (Finish::Timber, "timber"),
        (Finish::Grout, "grout"),
        (Finish::Substrate, "substrate"),
        (Finish::Asphalt, "asphalt"),
        (Finish::ManholeRim, "manhole-rim"),
        (Finish::ManholeCover, "manhole-cover"),
        (Finish::ManholeSlot, "manhole-slot"),
        (Finish::Crack, "crack"),
        (Finish::Joint, "joint"),
        (Finish::Spall, "spall"),
        (Finish::Chip, "chip"),
        (Finish::TileGrime, "tile-grime"),
    ];
    Assets {
        sources: BTreeMap::from(finishes.map(|(f, name)| (f, source(name)))),
        tile_bed: source("tile-bed"),
        edge: source("edge"),
        water: source("water-film"),
        paper: source("paper"),
        brick: source("brick"),
        can: source("can"),
    }
}
#[test]
fn complete_pavement_surveys_match_rendered_triangles_and_regions() {
    let assets = assets();
    // Panel 1 has a hole through its receiving faces; panel 3 draws flush.
    for i in 0..4 {
        let z = 0.1;
        let outline = field::rectangle(0., 0., 5., 4., z);
        let mut faces = Vec::new();
        let mut add = |px, py, w, d| {
            let p = field::rectangle(px, py, w, d, z);
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
        let mut budget = Budget::new(64_000_000, &|| false).unwrap();
        let grid = layout::footprint(
            &[outline],
            settings.patch_size,
            settings.material_size,
            &mut budget,
        )
        .unwrap();
        let walls = vec![
            Wall {
                host: "west".into(),
                seam: [[0.1, 0., z], [0.1, 4., z]],
            },
            Wall {
                host: "divider".into(),
                seam: [[1.8, 0., z], [1.8, 2.4, z]],
            },
        ];
        let seams = walls.iter().map(|w| w.seam).collect::<Vec<_>>();
        let host = format!("panel-{i}");
        let seed = format!("ground-render/{i}");
        let site = Site {
            host: &host,
            part: &host,
            seed: &seed,
            layout: &grid,
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
            density: Some(32.),
            asphalt_density: None,
            fine_marks: true,
            flush: i == 3,
        };
        let mut drawn = [Mode::Render, Mode::Survey].map(|mode| {
            let mut mesh = Mesh::new(mode, 500_000, &|| false).unwrap();
            let regions = render::draw_preserved(&mut mesh, &site, &assets, &mut budget).unwrap();
            let regions = regions
                .iter()
                .map(|r| json!({"id":r.id,"kind":r.kind,"bounds":r.bounds,"footprint":r.footprint}))
                .collect::<Vec<Value>>();
            (mesh, regions)
        });
        let [(render, regions), (survey, surveyed)] = &mut drawn;
        assert!(render.triangle_count() > 0, "{seed}");
        assert_eq!(regions, surveyed, "{seed} regions");
        assert_eq!(render.triangle_count(), survey.triangle_count(), "{seed}");
        assert!(survey.surveying());
        let corners = render
            .indices
            .iter()
            .map(|&i| render.vertices[i as usize].position)
            .collect::<Vec<_>>();
        assert_eq!(survey.survey.len(), corners.len());
        // Survey corners are stored at survey precision, not the rendered vertex precision.
        for (a, b) in survey.survey.iter().zip(&corners) {
            assert!(
                a.iter().zip(b).all(|(a, b)| (a - b).abs() <= 1e-5),
                "{seed}: {a:?} != {b:?}"
            );
        }
        assert!(
            render.vertices.iter().all(|v| v
                .position
                .iter()
                .chain(&v.normal)
                .chain(&v.uv)
                .all(|x| x.is_finite())),
            "{seed}"
        );
        weathered::finish(render, &|| false).unwrap();
    }
}
#[test]
fn empty_and_invalid_sites_emit_nothing() {
    let assets = assets();
    let settings = Settings::default();
    let mut budget = Budget::new(4_000_000, &|| false).unwrap();
    let outline = field::rectangle(0., 0., 4., 4., 0.);
    let grid = layout::footprint(&[outline.clone()], 1.9, 7., &mut budget).unwrap();
    let faces = vec![vec![outline[0], outline[1], outline[2]]];
    let draw = |host: &str, faces: &[Vec<[f64; 3]>], budget: &mut Budget<'_>| {
        let mut mesh = Mesh::new(Mode::Render, 100_000, &|| false).unwrap();
        let result = render::draw_preserved(
            &mut mesh,
            &Site {
                host,
                part: host,
                seed: "ground",
                layout: &grid,
                faces,
                seams: &[],
                walls: &[],
                settings: &settings,
                grime: 0.6,
                wear: 0.7,
                sampling: 0.3,
                tile_size: 0.4,
                tile_aspect: None,
                tile_wear: None,
                tile_grime: None,
                density: Some(32.),
                asphalt_density: None,
                fine_marks: false,
                flush: false,
            },
            &assets,
            budget,
        );
        (result.map(|r| r.len()), mesh.triangle_count())
    };
    assert_eq!(draw("ground", &[], &mut budget), (Ok(0), 0));
    assert!(draw("", &faces, &mut budget).0.is_err());
    assert!(
        draw("ground", &[vec![[f64::NAN; 3]; 3]], &mut budget)
            .0
            .is_err()
    );
    let (result, triangles) = draw("ground", &faces, &mut budget);
    assert!(result.is_ok() && triangles > 0);
}
