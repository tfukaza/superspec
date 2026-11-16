use std::{collections::BTreeMap, sync::Arc};
use superspec_engine::authoring::{Mapping, Receiver, Request, Session};
use superspec_geometry::image;
use superspec_model::instances::IDENTITY;

#[test]
fn catalog_preflight_follows_effective_item_slot_overrides() {
    use superspec_engine::materialspec::program::MaterialRequest;
    let mut request=Request {
        kind:"item".into(), entry:"pipe.item.json".into(),
        source:r#"{"format":"itemspec","version":1,"id":"pipe","generator":{"kind":"pipe","length":1,"radius":0.1},"slots":{"surface":{"entry":"saved.ms"}}}"#.into(),
        modules:BTreeMap::from([
            ("saved.ms".into(),"material 1; output base_color=sample_color(\"superspec-material:metal.png\",uv);".into()),
            ("replacement.ms".into(),"material 1; output base_color=sample_color(\"superspec-material:timber.png\",uv);".into()),
            ("unused.ms".into(),"material 1; output base_color=sample_color(\"superspec-material:rust.png\",uv);".into()),
        ]), ..Default::default()
    };
    let mut session = Session::default();
    assert_eq!(
        session.library_assets(&request),
        ["superspec-material:metal.png"]
    );
    request.slot_overrides.insert(
        "surface".into(),
        MaterialRequest {
            entry: "replacement.ms".into(),
            ..Default::default()
        },
    );
    assert_eq!(
        session.library_assets(&request),
        ["superspec-material:timber.png"]
    );
    request.slot_overrides.clear();
    assert_eq!(
        session.library_assets(&request),
        ["superspec-material:metal.png"]
    );
}

fn panel(id: &str, matrix: [f64; 16]) -> Receiver {
    let points = [[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]];
    let normal = [matrix[8], matrix[9], matrix[10]];
    Receiver {
        id: id.into(),
        object_id: id.into(),
        positions: points
            .into_iter()
            .flat_map(|p| {
                std::array::from_fn::<_, 3, _>(|i| {
                    matrix[i] * p[0] + matrix[i + 4] * p[1] + matrix[i + 8] * p[2] + matrix[i + 12]
                })
            })
            .collect(),
        normals: std::iter::repeat_n(normal, 4).flatten().collect(),
        uvs: vec![0., 0., 1., 0., 1., 1., 0., 1.],
        indices: vec![0, 1, 2, 0, 2, 3],
        object_to_world: Some(matrix),
        contacts: Vec::new(),
    }
}
fn request(source: &str, receiver: Receiver) -> Request {
    Request {
        source: source.into(),
        receivers: vec![receiver],
        resolution: 32,
        ..Request::default()
    }
}
fn image_bytes(value: u8) -> Arc<[u8]> {
    image::encode(&image::Image {
        width: 1,
        height: 1,
        rgba: vec![value, value, value, 255],
        mime: "image/png",
    })
    .unwrap()
    .into()
}
fn material_images(output: &superspec_engine::authoring::Output) -> BTreeMap<String, Vec<u8>> {
    output
        .model
        .images
        .iter()
        .map(|(id, image)| (id.clone(), image.bytes.to_vec()))
        .collect()
}

#[test]
fn object_projection_is_rigid_transform_invariant_and_world_projection_is_not() {
    let source = "material 1; output base_color=vec3(clamp(surface.x,0,1),clamp(surface.y,0,1),0.25); output roughness=0.7;";
    let rotated = [
        0., 1., 0., 0., 0., 0., 1., 0., 1., 0., 0., 0., 0.2, 0.3, 0.4, 1.,
    ];
    let original = panel("panel", IDENTITY);
    let placed = panel("panel", rotated);
    let mut session = Session::default();
    let a = session
        .build(
            &request(source, original.clone()),
            1,
            &|_| panic!("no source images"),
            &|| false,
        )
        .unwrap();
    let b = session
        .build(
            &request(source, placed.clone()),
            2,
            &|_| panic!("no source images"),
            &|| false,
        )
        .unwrap();
    assert_eq!(material_images(&a), material_images(&b));
    assert_eq!(
        b.model
            .meshes
            .values()
            .next()
            .unwrap()
            .positions
            .iter()
            .flatten()
            .copied()
            .collect::<Vec<_>>(),
        // The atlas lists each triangle from its chart corner (the one nearest a right angle), so
        // the placed mesh is the original's mesh carried by the rigid transform, vertex for vertex.
        a.model
            .meshes
            .values()
            .next()
            .unwrap()
            .positions
            .iter()
            .flat_map(|p| std::array::from_fn::<_, 3, _>(|i| {
                rotated[i] * p[0] + rotated[i + 4] * p[1] + rotated[i + 8] * p[2] + rotated[i + 12]
            }))
            .collect::<Vec<_>>()
    );
    // Independently of each other, both meshes are their receiver's own triangles, each listed
    // from its right-angle corner with the winding kept: (0, 1, 2) from vertex 1 and (0, 2, 3)
    // from vertex 3 of the unit panel.
    for (output, receiver) in [(&a, &original), (&b, &placed)] {
        assert_eq!(
            output
                .model
                .meshes
                .values()
                .next()
                .unwrap()
                .positions
                .iter()
                .flatten()
                .copied()
                .collect::<Vec<_>>(),
            [1usize, 2, 0, 3, 0, 2]
                .iter()
                .flat_map(|i| receiver.positions[i * 3..i * 3 + 3].iter().copied())
                .collect::<Vec<_>>()
        );
    }
    let world = Request {
        mapping: Mapping::World,
        ..request(source, placed)
    };
    let c = session
        .build(&world, 3, &|_| panic!("no source images"), &|| false)
        .unwrap();
    assert_ne!(material_images(&b), material_images(&c));
    assert_eq!(b.model.instances[0].object_id, "panel");
}

#[test]
fn image_dependency_updates_preserve_receiver_geometry_and_failed_edits_recover() {
    let source = "material 1; param tint: scalar=1 range(0,1); output base_color=sample_color(\"grime\",uv)*tint; output roughness=0.8;";
    let request = request(source, panel("receiver", IDENTITY));
    let dark = image_bytes(70);
    let light = image_bytes(190);
    let mut session = Session::default();
    let first = session
        .build(
            &request,
            1,
            &|name| {
                assert_eq!(name, "grime");
                Ok(dark.clone())
            },
            &|| false,
        )
        .unwrap();
    let again = session
        .build(&request, 2, &|_| Ok(dark.clone()), &|| false)
        .unwrap();
    for (id, image) in &first.model.images {
        assert!(Arc::ptr_eq(&image.bytes, &again.model.images[id].bytes));
    }
    let mesh = first.model.meshes.values().next().unwrap();
    assert!(Arc::ptr_eq(
        mesh,
        again.model.meshes.values().next().unwrap()
    ));
    let changed = session
        .build(&request, 3, &|_| Ok(light.clone()), &|| false)
        .unwrap();
    assert_ne!(material_images(&first), material_images(&changed));
    assert!(
        Arc::ptr_eq(mesh, changed.model.meshes.values().next().unwrap()),
        "a source image edit leaves prepared receiver geometry reusable"
    );
    let invalid = Request {
        source: "material 1; output base_color=unknown;".into(),
        ..request.clone()
    };
    let error = session
        .build(&invalid, 4, &|_| Ok(dark.clone()), &|| false)
        .err()
        .unwrap();
    assert_eq!(error[0].code, "MS_NAME");
    assert_eq!(error[0].source_name, "main.ms");
    assert!(
        session
            .build(&request, 5, &|_| Err("missing image".into()), &|| false)
            .is_err()
    );
    assert!(
        session
            .build(&request, 6, &|_| Ok(dark.clone()), &|| true)
            .is_err()
    );
    let recovered = session
        .build(&request, 7, &|_| Ok(dark.clone()), &|| false)
        .unwrap();
    assert_eq!(material_images(&first), material_images(&recovered));
    assert_eq!(
        mesh.positions,
        recovered.model.meshes.values().next().unwrap().positions
    );
    assert!(first.scene(1).unwrap().manifest.success);
}

#[test]
fn twenty_slopes_share_one_grime_input_and_clip_runoff_to_finite_edges() {
    let source = "material 1; let streak=runoff(0.08,2.0); output base_color=sample_color(\"grime\",vec2(world.x,world.z))*streak; output roughness=0.8;";
    let mut request = Request {
        source: source.into(),
        resolution: 32,
        ..Default::default()
    };
    request.mapping = Mapping::World;
    for index in 0..20 {
        let angle = index as f64 * std::f64::consts::FRAC_PI_2 / 19.;
        let (sin, cos) = angle.sin_cos();
        let x = 2. * index as f64;
        request.receivers.push(panel(
            &format!("slope-{index}"),
            [
                1., 0., 0., 0., 0., cos, sin, 0., 0., -sin, cos, 0., x, 0., 0., 1.,
            ],
        ));
        request
            .edges
            .push(superspec_engine::materialspec::program::Edge {
                start: [x, cos, sin],
                end: [x + 1., cos, sin],
            });
    }
    let grime = image_bytes(190);
    let before = grime.to_vec();
    let mut session = Session::default();
    let loads = std::cell::Cell::new(0);
    let output = session
        .build(
            &request,
            1,
            &|name| {
                assert_eq!(name, "grime");
                loads.set(loads.get() + 1);
                Ok(grime.clone())
            },
            &|| false,
        )
        .unwrap();
    assert_eq!(
        loads.get(),
        1,
        "all receivers resolve the same supplied image once"
    );
    assert_eq!(output.model.instances.len(), 20);
    for instance in &output.model.instances {
        let material = &output.model.materials[instance.material.as_ref().unwrap()];
        let image = &output.model.images[material.texture.as_ref().unwrap()];
        let decoded = image::decode(&image.bytes).unwrap();
        let covered = decoded.rgba.chunks_exact(4).any(|pixel| pixel[0] > 0);
        assert_eq!(
            covered,
            instance.id != "slope-0",
            "horizontal receivers have no gravity-driven runoff; other slopes do"
        );
    }
    request.edges.iter_mut().for_each(|edge| {
        edge.start[0] += 100.;
        edge.end[0] += 100.;
    });
    let outside = session
        .build(&request, 2, &|_| Ok(grime.clone()), &|| false)
        .unwrap();
    for instance in &outside.model.instances {
        let material = &outside.model.materials[instance.material.as_ref().unwrap()];
        let image = &outside.model.images[material.texture.as_ref().unwrap()];
        assert!(
            image::decode(&image.bytes)
                .unwrap()
                .rgba
                .chunks_exact(4)
                .all(|pixel| pixel[0] == 0)
        );
    }
    assert_eq!(grime.as_ref(), before);
}
