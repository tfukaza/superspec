use superspec_geometry::navigation::{Agent, build};

#[test]
fn tile_seams_connect_continuous_floors_and_preserve_real_gaps() {
    let agent = Agent {
        radius: 0.25,
        ..Default::default()
    };
    let vertices = [[0., 0., 0.], [80., 0., 0.], [80., 10., 0.], [0., 10., 0.]];
    let nav = build(&vertices, &[[0, 1, 2], [0, 2, 3]], agent, &|| false).unwrap();
    assert_eq!(nav.tile_count, 3);
    assert_eq!(nav.component_count(), 1);
    assert!(nav.path([2., 5., 0.], [78., 5., 0.], 0.25, 0.4).is_some());
    let vertices = [
        [0., 0., 0.],
        [39., 0., 0.],
        [39., 10., 0.],
        [0., 10., 0.],
        [41., 0., 0.],
        [80., 0., 0.],
        [80., 10., 0.],
        [41., 10., 0.],
    ];
    let nav = build(
        &vertices,
        &[[0, 1, 2], [0, 2, 3], [4, 5, 6], [4, 6, 7]],
        agent,
        &|| false,
    )
    .unwrap();
    assert!(nav.path([2., 5., 0.], [78., 5., 0.], 0.25, 0.4).is_none());
    let vertices = [
        [0., 0., 0.],
        [32., 0., 0.],
        [32., 10., 0.],
        [0., 10., 0.],
        [32., 0., 0.1],
        [64., 0., 0.1],
        [64., 10., 0.1],
        [32., 10., 0.1],
    ];
    let nav = build(
        &vertices,
        &[[0, 1, 2], [0, 2, 3], [4, 5, 6], [4, 6, 7]],
        Agent { step: 0., ..agent },
        &|| false,
    )
    .unwrap();
    assert!(nav.path([2., 5., 0.], [62., 5., 0.1], 0.25, 0.2).is_none());
}
