use superspec_aml::rng::SeededRng;
use superspec_engine::{Scene, compile, worldspec::themes::kowloon::floor::Finish};
use superspec_geometry::math::*;

const SOURCE: &str = include_str!("../../../tests/fixtures/aml/rust-floor-dressing.aml");
fn visible_area(scene: &Scene, material: Option<&str>) -> f64 {
    let mut area = 0.;
    for mesh in scene
        .manifest
        .meshes
        .iter()
        .filter(|m| m.visible && material.is_none_or(|id| m.material.as_deref() == Some(id)))
    {
        for t in scene.indices[mesh.index_start..mesh.index_start + mesh.index_count]
            .as_chunks::<3>()
            .0
        {
            let p = t.map(|i| {
                std::array::from_fn(|a| {
                    scene.positions[(mesh.vertex_start + i as usize) * 3 + a] as f64
                })
            });
            area += length(cross(sub(p[1], p[0]), sub(p[2], p[0]))) / 2.;
        }
    }
    area
}
#[test]
fn floor_colors_stay_in_range_and_grime_darkens_while_wear_lightens() {
    for seed in ["courtyard/地面🌿/floor", "a", "b/c"] {
        let phase = SeededRng::from_text(seed).float(0., 100.);
        assert!((0.0..100.0).contains(&phase));
        for point in [[0., 0., 0.], [1.3, -2.1, 0.4], [40., 7., -3.]] {
            let at = |grime, wear| {
                Finish {
                    grime,
                    wear,
                    phase,
                    repeat: 1.,
                }
                .color(point)
            };
            let base = at(0.5, 0.5);
            for channel in 0..3 {
                assert!((0.0..=1.0).contains(&base[channel]));
                assert!(at(1., 0.5)[channel] <= base[channel]);
                assert!(at(0.5, 1.)[channel] >= base[channel]);
            }
            assert!(base[2] <= base[0] && base[0] <= base[1], "warm grey order");
        }
    }
}

#[test]
fn floor_refinement_splits_long_edges_and_preserves_area_and_winding() {
    let finish = Finish {
        grime: 0.8,
        wear: 0.2,
        phase: SeededRng::from_text("courtyard/地面🌿/floor").float(0., 100.),
        repeat: 1.7,
    };
    let triangle = [[0., 0., 0.], [4., 0., 0.], [4., 3., 0.]];
    let mut output = Vec::new();
    let mut work = 0;
    finish
        .triangle(triangle, [0., 0., 1.], &mut work, &mut output)
        .unwrap();
    assert!(work >= output.len() / 3);
    let mut area = 0.;
    for t in output.as_chunks::<3>().0 {
        let p = t.each_ref().map(|v| v.position);
        let n = cross(sub(p[1], p[0]), sub(p[2], p[0]));
        assert!(n[2] > 0.);
        area += length(n) / 2.;
        for i in 0..3 {
            assert!(length(sub(p[i], p[(i + 1) % 3])) <= 0.650001);
        }
        for v in t {
            assert_eq!(v.normal, [0., 0., 1.]);
            assert_eq!(v.color, finish.color(v.position));
        }
    }
    assert!((area - 6.).abs() < 1e-9);
}

#[test]
fn floor_finish_preserves_physics_inlays_area_and_bounds() {
    let plain_source = SOURCE.split("  <dressing").next().unwrap().to_owned() + "</architecture>";
    let plain = compile(&plain_source, 0).unwrap();
    let dressed = compile(SOURCE, 0).unwrap();
    let collision = |scene: &Scene| {
        scene
            .manifest
            .meshes
            .iter()
            .filter(|m| scene.manifest.colliders.contains(&m.id))
            .map(|m| {
                (
                    m.id.clone(),
                    scene.positions[m.vertex_start * 3..(m.vertex_start + m.vertex_count) * 3]
                        .to_vec(),
                    scene.indices[m.index_start..m.index_start + m.index_count].to_vec(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(collision(&plain), collision(&dressed));
    assert!((visible_area(&dressed, None) - visible_area(&plain, None)).abs() < 1e-6);
    assert!((visible_area(&dressed, Some("finish")) - 11.).abs() < 1e-6);
    assert!((visible_area(&dressed, Some("inlay")) - 1.).abs() < 1e-6);
    let untouched = |scene: &Scene| {
        let mut values = Vec::new();
        for mesh in scene
            .manifest
            .meshes
            .iter()
            .filter(|m| m.visible && m.material.as_deref() != Some("finish"))
        {
            for i in &scene.indices[mesh.index_start..mesh.index_start + mesh.index_count] {
                let i = mesh.vertex_start + *i as usize;
                // Plain top receivers are the only surfaces expected to be replaced.
                if mesh.material.as_deref() != Some("inlay") && scene.normals[i * 3 + 2] > 0.7 {
                    continue;
                }
                values.push((
                    mesh.material.clone(),
                    scene.positions[i * 3..i * 3 + 3]
                        .iter()
                        .chain(&scene.normals[i * 3..i * 3 + 3])
                        .chain(&scene.uvs[i * 2..i * 2 + 2])
                        .chain(&scene.colors[i * 3..i * 3 + 3])
                        .map(|v| v.to_bits())
                        .collect::<Vec<_>>(),
                ));
            }
        }
        values.sort();
        values
    };
    assert_eq!(untouched(&plain), untouched(&dressed));
    for mesh in dressed
        .manifest
        .meshes
        .iter()
        .filter(|m| m.material.as_deref() == Some("finish"))
    {
        assert_eq!(mesh.object_id, "floor");
        assert_eq!(mesh.bounds, [[0., 0., 0.], [4., 3., 0.]]);
        for t in dressed.indices[mesh.index_start..mesh.index_start + mesh.index_count]
            .as_chunks::<3>()
            .0
        {
            let p = t.map(|i| {
                std::array::from_fn(|a| {
                    dressed.positions[(mesh.vertex_start + i as usize) * 3 + a] as f64
                })
            });
            assert!(cross(sub(p[1], p[0]), sub(p[2], p[0]))[2] > 0.);
            for i in 0..3 {
                assert!(length(sub(p[i], p[(i + 1) % 3])) <= 0.650001);
            }
        }
    }
}

#[test]
fn integrated_floor_uses_dressing_seed_material_and_rule_overrides() {
    let phase = SeededRng::from_text("courtyard/地面🌿/floor").float(0., 100.);
    for (source, grime, wear) in [
        (SOURCE.to_owned(), 0.8, 0.2),
        (
            SOURCE.replace("<floor host", "<floor grime=\"0\" wear=\"1\" host"),
            0.,
            1.,
        ),
    ] {
        let finish = Finish {
            grime,
            wear,
            phase,
            repeat: 1.7,
        };
        let scene = compile(&source, 0).unwrap();
        for position in [[0., 0., 0.], [4., 0., 0.], [4., 3., 0.], [0., 3., 0.]] {
            // The producer's own corner at this point, from a tiny triangle anchored there.
            let mut produced = Vec::new();
            finish
                .triangle(
                    [
                        position,
                        add(position, [0.01, 0., 0.]),
                        add(position, [0., 0.01, 0.]),
                    ],
                    [0., 0., 1.],
                    &mut 0,
                    &mut produced,
                )
                .unwrap();
            let reference = &produced[0];
            let mut found = false;
            for mesh in scene
                .manifest
                .meshes
                .iter()
                .filter(|m| m.material.as_deref() == Some("finish"))
            {
                for i in mesh.vertex_start..mesh.vertex_start + mesh.vertex_count {
                    if scene.positions[i * 3..i * 3 + 3] != position.map(|v| v as f32) {
                        continue;
                    }
                    found = true;
                    assert_eq!(
                        scene.normals[i * 3..i * 3 + 3],
                        reference.normal.map(|v| v as f32)
                    );
                    assert_eq!(scene.uvs[i * 2..i * 2 + 2], reference.uv.map(|v| v as f32));
                    assert_eq!(
                        scene.colors[i * 3..i * 3 + 3],
                        reference.color.map(|v| v as f32)
                    );
                }
            }
            assert!(found);
        }
    }
}

#[test]
fn floor_uv_projection_turns_and_work_admission() {
    let finish = Finish {
        grime: 0.,
        wear: 0.,
        phase: 0.,
        repeat: 2.,
    };
    for phase in 0..4 {
        let finish = Finish {
            phase: phase as f64,
            ..finish.clone()
        };
        for (normal, axes) in [
            ([0., 0., 1.], [0, 1]),
            ([0., 1., 0.], [0, 2]),
            ([1., 0., 0.], [1, 2]),
        ] {
            let p = [0.2, 0.3, 0.4];
            let mut a = p;
            let mut b = p;
            a[axes[0]] += 0.1;
            b[axes[1]] += 0.1;
            let mut output = Vec::new();
            finish
                .triangle([p, a, b], normal, &mut 0, &mut output)
                .unwrap();
            let u = p[axes[0]] * 2.;
            let v = p[axes[1]] * 2.;
            let rotated = match phase {
                0 => [u, v],
                1 => [-v, u],
                2 => [-u, -v],
                _ => [v, -u],
            };
            assert_eq!(
                output[0].uv,
                [rotated[0] + phase as f64, rotated[1] + phase as f64 * 0.37]
            );
        }
    }
    let triangle = [[2., 0., 0.], [2.1, 0., 0.], [2., 0.1, 0.]];
    let mut output = Vec::new();
    for mut work in [2_000_000, usize::MAX] {
        assert!(
            finish
                .triangle(triangle, [0., 0., 1.], &mut work, &mut output)
                .is_err()
        );
    }
    assert!(
        Finish {
            repeat: 1e308,
            ..finish.clone()
        }
        .triangle(triangle, [0., 0., 1.], &mut 0, &mut output)
        .is_err()
    );
    assert!(output.is_empty());
    finish
        .triangle(triangle, [0., 0., 1.], &mut 0, &mut output)
        .unwrap();
    assert_eq!(output.len(), 3);
}

#[test]
fn admission_is_explicit_and_collision_only_never_reads_visual_assets() {
    for (from, to) in [
        ("host=\"floor\"", "host=\"missing\""),
        ("grime=\"0.8\"", "grime=\"NaN\""),
        ("host=\"floor\"", "host=\"floor\" windows=\"true\""),
        ("host=\"floor\"", "host=\"floor\" ground=\"patched\""),
        ("<floor host", "<building host"),
        ("<floor host", "<wall host"),
        ("<dressing seed", "<dressing unexpected=\"1\" seed"),
    ] {
        assert!(compile(&SOURCE.replace(from, to), 0).is_err(), "{to}");
    }
    let duplicate = SOURCE.replace(
        "</dressing>",
        "<floor host=\"floor\" material=\"finish\"/></dressing>",
    );
    let error = compile(&duplicate, 0).unwrap_err();
    assert_eq!(error.objects, ["floor"]);
    let textured = SOURCE.replace("repeat=\"1.7\"", "repeat=\"1.7\" texture=\"missing.png\"");
    assert!(compile(&textured, 0).is_err());
    superspec_engine::compile_collision_with_assets(&textured, 0, &|_| {
        panic!("visual image requested by collision")
    })
    .unwrap();
    assert!(compile(SOURCE, 0).is_ok());
}

#[test]
fn reflected_instanced_floor_selects_local_top_and_maps_world_coordinates() {
    let source = r##"<architecture version="1" id="tilt">
      <material id="finish" color="#c6c2b4" repeat="1.7"/>
      <profile id="plan" points="0 0;2 0;2 1;0 1"/>
      <definition id="unit"><slab id="floor" profile="plan" thickness="0.2"/></definition>
      <instance id="placed" ref="unit" position="5 2 3" rotation="90 0 0" scale="-2 1.5 1"/>
      <dressing seed="tilt" grime="0.8" wear="0.2"><floor host="placed/floor" material="finish"/></dressing>
    </architecture>"##;
    let scene = compile(source, 0).unwrap();
    assert!((visible_area(&scene, Some("finish")) - 6.).abs() < 1e-6);
    let phase = SeededRng::from_text("tilt/placed/floor").float(0., 100.);
    let angle = phase.floor() * std::f64::consts::FRAC_PI_2;
    let (s, c) = (angle.sin().round(), angle.cos().round());
    let mut corner = false;
    for mesh in scene
        .manifest
        .meshes
        .iter()
        .filter(|m| m.material.as_deref() == Some("finish"))
    {
        assert_eq!(mesh.object_id, "placed/floor");
        for i in mesh.vertex_start..mesh.vertex_start + mesh.vertex_count {
            let p = &scene.positions[i * 3..i * 3 + 3];
            assert!((p[1] - 2.).abs() < 1e-6);
            assert_eq!(scene.normals[i * 3..i * 3 + 3], [0., -1., 0.]);
            if p == [5., 2., 3.] {
                corner = true;
                let (u, v) = (5. * 1.7, 3. * 1.7);
                assert_eq!(
                    scene.uvs[i * 2..i * 2 + 2],
                    [
                        (u * c - v * s + phase) as f32,
                        (u * s + v * c + phase * 0.37) as f32
                    ]
                );
            }
        }
    }
    assert!(corner);
}
