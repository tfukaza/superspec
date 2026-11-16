//! Field bakes: the CPU values a GPU preview binds as textures for the parts of a program only the
//! CPU can evaluate, taken over the same atlas a full bake uses.
use std::sync::Arc;
use superspec_engine::authoring::{Request, Session};
use superspec_engine::materialspec::program::Edge;

fn request(edges: Vec<Edge>) -> Request {
    let source = "material 1;
        param width: scalar = 0.2 range(0.01, 1);
        let wet = runoff(width, 0.7);
        let plain = noise(surface * 4.0, seed(\"plain\"));
        output base_color = vec3(plain * (1.0 - wet));
        output roughness = 0.5;
        output metalness = 0;";
    Request {
        entry: "main.ms".into(),
        source: source.into(),
        resolution: 64,
        domain: [1., 1.],
        edges,
        gravity: [0., 0., -1.],
        ..Request::default()
    }
}

fn assets(_: &str) -> Result<Arc<[u8]>, String> {
    Err("no assets".into())
}

#[test]
fn a_field_bake_covers_the_atlas_and_follows_the_edges() {
    let mut session = Session::default();
    let names = ["wet".to_owned()];
    let empty = session
        .bake_fields(&request(vec![]), 1, &names, &assets, &|| false)
        .unwrap();
    assert_eq!((empty.width, empty.height), (64, 64));
    assert_eq!(empty.fields.len(), 1);
    assert_eq!(empty.fields[0].len(), 64 * 64 * 4);
    assert!(
        empty.fields[0].iter().all(|v| *v == 0.),
        "without source edges there is no runoff"
    );
    // A finite edge across the swatch's top (the swatch spans x 0..1, z 0..1 with Z up) wets the
    // surface below it.
    let edge = Edge {
        start: [0.2, 0., 1.],
        end: [0.8, 0., 1.],
    };
    let wet = session
        .bake_fields(&request(vec![edge]), 2, &names, &assets, &|| false)
        .unwrap();
    let texels = &wet.fields[0];
    let values: Vec<f32> = texels.chunks(4).map(|texel| texel[0]).collect();
    assert!(values.iter().all(|v| (0. ..=1.).contains(v)), "a mask");
    assert!(
        values.iter().any(|v| *v > 0.5),
        "runoff reaches texels under the edge"
    );
    assert!(values.iter().any(|v| *v == 0.), "and leaves others dry");
    // Rows count from the atlas's upper edge, which is the swatch's top for this layout: the wet
    // texels sit in the upper rows, not the lower ones.
    let rows = |range: std::ops::Range<usize>| {
        range
            .map(|row| values[row * 64..(row + 1) * 64].iter().sum::<f32>())
            .sum::<f32>()
    };
    assert!(
        rows(2..30) != rows(34..62),
        "runoff is not uniform down the swatch"
    );
    // A parameter the field reads changes its texels.
    let mut wide = request(vec![Edge {
        start: [0.2, 0., 1.],
        end: [0.8, 0., 1.],
    }]);
    wide.parameters.insert("width".into(), 0.4.into());
    let wider = session
        .bake_fields(&wide, 4, &names, &assets, &|| false)
        .unwrap();
    let total = |bake: &superspec_engine::authoring::FieldBake| -> f32 {
        bake.fields[0].chunks(4).map(|texel| texel[0]).sum()
    };
    assert_ne!(total(&wider), total(&wet), "the width parameter reaches the field");
    // A field that is not in the program is refused.
    assert!(
        session
            .bake_fields(
                &request(vec![]),
                3,
                &["missing".to_owned()],
                &assets,
                &|| false
            )
            .is_err()
    );
}
