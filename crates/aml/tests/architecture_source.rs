use superspec_aml::architecture::{parse, quantize};

#[test]
fn authored_millimeter_ties_are_symmetric_and_reference_cameras_keep_precision() {
    for (input, expected) in [
        (0.0005, 0.001),
        (-0.0005, -0.001),
        (1.0015, 1.002),
        (-1.0015, -1.002),
        (0.00049, 0.),
        (-0.00049, 0.),
        (1.0004, 1.),
    ] {
        assert_eq!(quantize(input), expected);
    }
    let doc=parse(r#"<architecture version="1" id="x"><reference id="r" image="a.png" width="1" height="1" scale-note="test" position="0.123456 0 2" target="0 0 0"/></architecture>"#).unwrap();
    assert_eq!(
        doc.metadata[0].vector("position", [0.; 3]),
        [0.123456, 0., 2.]
    );
}
