use superspec_engine::{compile, updates};

#[test]
fn retained_meshes_require_exact_buffers_and_changed_ranges_are_delivered() {
    let source = "<architecture version=\"1\" id=\"update\"><box id=\"a\"/><box id=\"b\" position=\"4 0 0\"/></architecture>";
    let original = compile(source, 1).unwrap();
    let edited = compile(&source.replace("id=\"a\"/>", "id=\"a\" width=\"2\"/>"), 2).unwrap();
    let first = updates::encode(&original, None);
    let next = updates::encode(&edited, Some(&original));
    let length = u32::from_le_bytes(next[4..8].try_into().unwrap()) as usize;
    let header: serde_json::Value = serde_json::from_slice(&next[8..8 + length]).unwrap();
    assert_eq!(header["baseRevision"], 1);
    let refs = header["meshes"].as_array().unwrap();
    for (i, mesh) in edited.manifest.meshes.iter().enumerate() {
        assert_eq!(refs[i].is_null(), mesh.object_id == "a");
    }
    assert!(next.len() < first.len());
    let same = updates::encode(&edited, Some(&edited));
    let length = u32::from_le_bytes(same[4..8].try_into().unwrap()) as usize;
    assert_eq!(same.len(), 8 + length);
}
