//! Real compiler/producer fixture shared by native integration and browser Wasm acceptance.
use std::collections::BTreeMap;
use superspec_engine::{
    Scene, compile_with_assets,
    worldspec::themes::kowloon::{
        mesh::{Mesh, Mode},
        pen::{Pen, Tint},
    },
};
use superspec_geometry::{
    Matrix,
    image::{Image, encode},
};

pub fn architecture() -> Scene {
    let image = encode(&Image {
        width: 2,
        height: 2,
        rgba: vec![
            231, 48, 72, 255, 92, 183, 56, 255, 17, 35, 201, 255, 255, 255, 255, 255,
        ],
        mime: "image/png",
    })
    .unwrap();
    compile_with_assets(r##"<architecture version="1" id="procedural-preview">
    <material id="finish" color="#ffffff" texture="detail.png"/>
    <definition id="kit"><box id="host" width="1" depth="1" height="1" material="finish"/></definition>
    <instance id="placement" ref="kit"/>
    </architecture>"##,71,&|path|if path=="detail.png"{Ok(image.clone())}else{Err("unknown fixture asset".into())}).unwrap()
}
pub fn drawing() -> Mesh<'static> {
    let mut mesh = Mesh::new(Mode::Render, 100, &|| false).unwrap();
    let pen = Pen::new(Matrix::default(), "assembly").unwrap();
    pen.cuboid(
        &mut mesh,
        "paint",
        [1.3, 0., 0.],
        [2.3, 1., 1.],
        &Tint::Color([0.25, 0.5, 1.5]),
        1.,
        "",
    )
    .unwrap();
    pen.into_part("rail")
        .unwrap()
        .beveled_bar(
            &mut mesh,
            "metal",
            [2.7, 0.5, 0.],
            [2.7, 0.5, 1.5],
            [1., 0., 0.],
            0.3,
            0.3,
            0.01,
            &Tint::Color([0.75, 0., 0.25]),
            1.,
        )
        .unwrap();
    // Return to the first group after another group emitted vertices.
    pen.cuboid(
        &mut mesh,
        "paint",
        [1.5, 0.2, 1.],
        [2.1, 0.8, 1.2],
        &Tint::Color([0.25, 0.5, 1.5]),
        1.,
        "",
    )
    .unwrap();
    mesh
}
pub fn bindings() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("paint".into(), "finish".into()),
        ("metal".into(), "finish".into()),
    ])
}
pub fn scene() -> Scene {
    let mut scene = architecture();
    scene
        .append_visual("placement/host", &drawing(), &bindings())
        .unwrap();
    scene
}
