//! Bakes that read planes bound to a program's external inputs: the planes enter map keys by
//! content, reach every evaluator (serial, threaded, other sessions' shares, field bakes), and a
//! failed or cancelled bake leaves nothing behind.
use super::*;
use crate::authoring::{Profile, Request};
use std::cell::Cell;

/// The fallbacks are the values the constant planes of these tests hold, so a bake with planes
/// can be compared with a bake that reads no plane at all.
fn source(shade: f64, rough: f64, lift: f64) -> String {
    format!(
        "material 1;
param tint: scalar = 0.5 range(0, 1);
let shade = external(\"shade\", {shade:?});
let grain = noise(surface * 12, seed(\"g\"));
output base_color = vec3(tint * shade);
output roughness = external(\"rough\", {rough:?});
output metalness = 0;
output height = 0.002 * grain * external(\"lift\", {lift:?});"
    )
}

fn request(source: String, resolution: usize) -> Request {
    Request {
        entry: "main.ms".into(),
        source,
        resolution,
        ..Request::default()
    }
}

/// Assets by name, each one allocation for as long as the host keeps it.
#[derive(Default)]
struct Assets(BTreeMap<String, Arc<[u8]>>);

impl Assets {
    fn get(&self, name: &str) -> Result<Arc<[u8]>, String> {
        self.0
            .get(name)
            .cloned()
            .ok_or_else(|| format!("{name} is unavailable"))
    }
    fn plane(&mut self, name: &str, atlas: &Atlas, value: impl Fn(usize) -> f32) {
        let values: Vec<f32> = (0..atlas.width * atlas.height).map(value).collect();
        let bytes = program::Plane::encode_external(atlas.width, atlas.height, 1, &values).unwrap();
        self.0.insert(name.to_owned(), bytes.into());
    }
}

fn swatch_atlas(request: &Request) -> Atlas {
    Atlas::for_request(receiver::swatch(request.domain), request).unwrap()
}

/// Binds the swatch's external inputs to assets of the same names with a suffix.
fn bind(request: &mut Request, names: &[&str], suffix: &str) {
    let bound = request.planes.entry("studio/swatch".into()).or_default();
    for name in names {
        bound.insert((*name).to_owned(), format!("{name}{suffix}"));
    }
}

fn build(
    session: &mut Session,
    revision: u64,
    request: &Request,
    assets: &Assets,
) -> (Output, Vec<Groups>) {
    BAKED.with(|baked| baked.borrow_mut().clear());
    let output = session
        .build(request, revision, &|name| assets.get(name), &|| false)
        .unwrap_or_else(|e| panic!("{e:?}"));
    (
        output,
        BAKED.with(|baked| std::mem::take(&mut *baked.borrow_mut())),
    )
}

fn fresh(request: &Request, assets: &Assets) -> Output {
    build(&mut Session::default(), 1, request, assets).0
}

fn assert_same(a: &Output, b: &Output, what: &str) {
    assert_eq!(
        a.model.materials.keys().collect::<Vec<_>>(),
        b.model.materials.keys().collect::<Vec<_>>(),
        "{what}: material identities hash every map"
    );
    assert_eq!(a.model.images.len(), b.model.images.len(), "{what}");
    for (id, image) in &a.model.images {
        assert_eq!(&*image.bytes, &*b.model.images[id].bytes, "{what}: {id}");
    }
}

/// Decoded pixels of every map of the one material, by role.
fn pixels(output: &Output) -> BTreeMap<&'static str, Vec<u8>> {
    let material = output.model.materials.values().next().unwrap();
    [
        ("color", &material.texture),
        ("normal", &material.normal_texture),
        ("properties", &material.metallic_roughness_texture),
    ]
    .into_iter()
    .map(|(role, id)| {
        let bytes = &output.model.images[id.as_ref().unwrap()].bytes;
        (role, image::decode(bytes).unwrap().rgba)
    })
    .collect()
}

/// Each group baked, however the bakes were split.
fn baked_once(baked: &[Groups]) -> Groups {
    let mut all = Groups::NONE;
    for groups in baked {
        for group in groups.iter() {
            assert!(!all.contains(group), "{group:?} was baked twice: {baked:?}");
            all = all.with(group);
        }
    }
    all
}

fn groups(list: &[Group]) -> Groups {
    list.iter()
        .fold(Groups::NONE, |set, group| set.with(*group))
}

#[test]
fn constant_planes_bake_what_the_same_constants_bake_without_planes() {
    let mut with = request(source(0.5, 0.25, 1.), 48);
    let atlas = swatch_atlas(&with);
    let mut assets = Assets::default();
    assets.plane("shade.plane", &atlas, |_| 0.75);
    assets.plane("rough.plane", &atlas, |_| 0.625);
    assets.plane("lift.plane", &atlas, |_| 0.5);
    bind(&mut with, &["shade", "rough", "lift"], ".plane");
    // A name the program does not read is never fetched.
    bind(&mut with, &["occlusion"], ".missing");
    let mut session = Session::default();
    let (planes, baked) = build(&mut session, 1, &with, &assets);
    assert_eq!(baked_once(&baked), Groups::MAPS);
    let constants = request(source(0.75, 0.625, 0.5), 48);
    assert_same(&planes, &fresh(&constants, &assets), "planes of constants");
    // Unbound, the inputs read their fallbacks.
    let unbound = request(source(0.5, 0.25, 1.), 48);
    let (fallback, _) = build(&mut session, 2, &unbound, &assets);
    assert_ne!(
        fallback.model.materials.keys().collect::<Vec<_>>(),
        planes.model.materials.keys().collect::<Vec<_>>()
    );
    assert_same(
        &fallback,
        &fresh(&request(source(0.5, 0.25, 1.), 48), &Assets::default()),
        "fallbacks",
    );
}

#[test]
fn each_texel_reads_its_own_value_of_a_plane() {
    let mut with = request(source(0.5, 0.25, 1.), 64);
    let atlas = swatch_atlas(&with);
    let mut assets = Assets::default();
    // Exact bytes: texel t holds (t mod 251) / 255.
    let level = |texel: usize| (texel % 251) as u8;
    assets.plane("rough.plane", &atlas, |texel| {
        f32::from(level(texel)) / 255.
    });
    bind(&mut with, &["rough"], ".plane");
    let output = fresh(&with, &assets);
    let properties = &pixels(&output)["properties"];
    let (mut own, mut total) = (0, 0);
    for chart in &atlas.charts {
        for y in 0..chart.height {
            for x in 0..chart.width {
                let texel = (chart.y + y) * atlas.width + chart.x + x;
                let roughness = properties[texel * 4 + 1];
                total += 1;
                if roughness == level(texel) {
                    own += 1;
                } else {
                    // A texel that samples the point of the texel before it repeats that texel.
                    assert!(
                        x > 0,
                        "texel {x},{y} of a chart's first column reads its own"
                    );
                    assert_eq!(roughness, properties[(texel - 1) * 4 + 1], "texel {x},{y}");
                    assert_eq!(
                        atlas.texel(chart, x, y),
                        atlas.texel(chart, x - 1, y),
                        "texel {x},{y} repeats only an equal sample"
                    );
                }
            }
        }
    }
    assert!(
        total >= 64 * 32 && own * 10 >= total * 8,
        "{own} of {total}"
    );
}

#[test]
fn a_plane_edit_bakes_only_the_maps_that_read_it() {
    let mut request = request(source(0.5, 0.25, 1.), 48);
    let atlas = swatch_atlas(&request);
    let mut assets = Assets::default();
    let ramp = |scale: f32| move |texel: usize| (texel % 97) as f32 / 97. * scale;
    assets.plane("shade.a", &atlas, ramp(1.));
    assets.plane("rough.a", &atlas, ramp(0.5));
    assets.plane("lift.a", &atlas, ramp(0.25));
    bind(&mut request, &["shade", "rough", "lift"], ".a");
    let mut session = Session::default();
    let (first, baked) = build(&mut session, 1, &request, &assets);
    // The three maps read three different planes, so no bake evaluates two of them; each map
    // is still baked once and equals a fresh bake.
    assert_eq!(baked_once(&baked), Groups::MAPS);
    assert_same(&first, &fresh(&request, &assets), "first bake");
    use Group::*;
    let mut revision = 1;
    for (name, expected) in [("rough", Properties), ("shade", Color), ("lift", Normal)] {
        assets.plane(&format!("{name}.b"), &atlas, ramp(0.125));
        let mut edited = request.clone();
        bind(&mut edited, &[name], ".b");
        revision += 1;
        let (output, baked) = build(&mut session, revision, &edited, &assets);
        assert_eq!(baked, [groups(&[expected])], "a new {name} plane");
        assert_same(&output, &fresh(&edited, &assets), name);
        // Equal content under another asset name and allocation is the same plane.
        let copy: Arc<[u8]> = assets.0[&format!("{name}.b")].to_vec().into();
        assets.0.insert(format!("{name}.c"), copy);
        bind(&mut edited, &[name], ".c");
        revision += 1;
        let (again, baked) = build(&mut session, revision, &edited, &assets);
        assert!(baked.is_empty(), "equal content bakes nothing: {baked:?}");
        assert_same(&again, &output, "equal content");
        // Unbinding reads the fallback: again only that map.
        let mut unbound = request.clone();
        unbound
            .planes
            .get_mut("studio/swatch")
            .unwrap()
            .remove(name);
        revision += 1;
        let (output, baked) = build(&mut session, revision, &unbound, &assets);
        assert_eq!(baked, [groups(&[expected])], "{name} unbound");
        assert_same(&output, &fresh(&unbound, &assets), "unbound");
        revision += 1;
        let (back, baked) = build(&mut session, revision, &request, &assets);
        assert!(baked.is_empty(), "{baked:?}");
        assert_same(&back, &first, "back to the first planes");
    }
}

#[test]
fn threads_blocks_and_other_sessions_bake_the_same_texels_from_planes() {
    let mut preview = request(source(0.5, 0.25, 1.), 96);
    let atlas = swatch_atlas(&preview);
    let mut assets = Assets::default();
    // Planes that differ at every texel, the relief's among them: a block, thread or share that
    // read a neighbor row's plane values at other texels would bake another normal.
    let varied =
        |salt: usize| move |texel: usize| ((texel * 2_654_435_761 + salt) % 1009) as f32 / 1009.;
    assets.plane("shade.plane", &atlas, varied(1));
    assets.plane("rough.plane", &atlas, varied(2));
    assets.plane("lift.plane", &atlas, varied(3));
    bind(&mut preview, &["shade", "rough", "lift"], ".plane");
    let local = fresh(&preview, &assets);
    let expected = pixels(&local);
    // Production splits rows across threads (and compresses harder: same pixels, other bytes).
    let production = Request {
        profile: Profile::Production,
        ..preview.clone()
    };
    assert_eq!(pixels(&fresh(&production, &assets)), expected, "threads");
    // Other sessions evaluate the rows, with the planes the pending bake names.
    for shares in [1, 3, 7] {
        let mut session = Session::default();
        session.distribute_bakes(true);
        let stopped = session
            .build(&preview, 1, &|name| assets.get(name), &|| false)
            .err()
            .unwrap();
        assert_eq!(stopped[0].code, "BAKE_PENDING");
        let pending = session.take_pending_bake().expect("a pending bake");
        assert_eq!(pending.planes, ["lift.plane", "rough.plane", "shade.plane"]);
        let pending: PendingBake =
            serde_json::from_str(&serde_json::to_string(&pending).unwrap()).unwrap();
        assert_eq!(pending.planes.len(), 3, "the description carries them");
        for index in 0..shares {
            let share = Session::default()
                .bake_share(&pending, index, shares, &|name| assets.get(name), &|| false)
                .unwrap_or_else(|e| panic!("{e:?}"));
            session.provide_bake_share(&share).unwrap();
        }
        let (distributed, baked) = build(&mut session, 2, &preview, &assets);
        assert_eq!(
            baked,
            [Groups::MAPS],
            "provided rows serve every map at once"
        );
        assert_same(&distributed, &local, &format!("{shares} shares"));
    }
    // A session without the plane cannot bake a share of it.
    let mut session = Session::default();
    session.distribute_bakes(true);
    let _ = session.build(&preview, 1, &|name| assets.get(name), &|| false);
    let pending = session.take_pending_bake().unwrap();
    let errors = Session::default()
        .bake_share(&pending, 0, 2, &|name| Assets::default().get(name), &|| {
            false
        })
        .err()
        .expect("a share without its planes fails");
    assert!(errors[0].message.contains("lift.plane"), "{errors:?}");
}

#[test]
fn bad_planes_and_cancellation_publish_nothing_and_later_builds_recover() {
    let mut request = request(source(0.5, 0.25, 1.), 32);
    let atlas = swatch_atlas(&request);
    let mut assets = Assets::default();
    assets.plane("shade.plane", &atlas, |texel| (texel % 13) as f32 / 13.);
    assets.plane("rough.plane", &atlas, |texel| (texel % 7) as f32 / 7.);
    assets.plane("lift.plane", &atlas, |texel| (texel % 5) as f32 / 5.);
    bind(&mut request, &["shade", "rough", "lift"], ".plane");
    let expected = fresh(&request, &assets);
    let mut session = Session::default();
    let mut revision = 0;
    let mut fails = |session: &mut Session, request: &Request, assets: &Assets, fragment: &str| {
        revision += 1;
        let errors = session
            .build(request, revision, &|name| assets.get(name), &|| false)
            .err()
            .expect("a bad plane fails the build");
        assert!(errors[0].message.contains(fragment), "{errors:?}");
    };
    // Bytes for another atlas, bytes that are no plane, two components for a scalar, a value that
    // is not finite, and an asset the host does not have.
    let mut bad = Assets(assets.0.clone());
    let other = swatch_atlas(&Request {
        resolution: 64,
        ..request.clone()
    });
    bad.plane("shade.plane", &other, |_| 0.5);
    fails(&mut session, &request, &bad, "'shade' covers");
    bad.0
        .insert("shade.plane".into(), (*b"not a plane at all").into());
    fails(
        &mut session,
        &request,
        &bad,
        "'shade' is not an encoded plane",
    );
    let pairs = vec![0.5f32; atlas.width * atlas.height * 2];
    let bytes = program::Plane::encode_external(atlas.width, atlas.height, 2, &pairs).unwrap();
    bad.0.insert("shade.plane".into(), bytes.into());
    fails(
        &mut session,
        &request,
        &bad,
        "'shade' holds 2 components where the program reads 1",
    );
    bad.plane("shade.plane", &atlas, |texel| {
        if texel == 9 { f32::INFINITY } else { 0.5 }
    });
    fails(
        &mut session,
        &request,
        &bad,
        "'shade' holds a non-finite value",
    );
    bad.0.remove("shade.plane");
    fails(&mut session, &request, &bad, "shade.plane is unavailable");
    // Cancelling at every poll in turn never leaves a partial result behind.
    let mut cancelled_builds = 0;
    for limit in 0..40 {
        revision += 1;
        let polls = Cell::new(0);
        let outcome = session.build(&request, revision, &|name| assets.get(name), &|| {
            polls.set(polls.get() + 1);
            polls.get() > limit
        });
        match outcome {
            Ok(output) => assert_same(&output, &expected, "finished before the cancellation"),
            Err(errors) => {
                cancelled_builds += 1;
                assert!(
                    errors[0].message.contains("cancel")
                        || errors[0].message.contains("superseded"),
                    "{errors:?}"
                );
            }
        }
    }
    assert!(cancelled_builds > 0, "some polls must cancel the build");
    revision += 1;
    let (recovered, _) = build(&mut session, revision, &request, &assets);
    assert_same(&recovered, &expected, "after failures and cancellations");
}

/// What handing planes to other sessions costs against baking where the planes already are:
/// `cargo test --release -p superspec-engine --lib bench_planes_under_distribution -- --ignored
/// --nocapture`. A share pays for its planes once (digest and decode); the rows it evaluates are
/// the same work wherever they run.
#[test]
#[ignore]
fn bench_planes_under_distribution() {
    let heavy = "material 1;
param tint: scalar = 0.5 range(0, 1);
let occlusion = external(\"occlusion\", 1.0);
let heavy = fbm(surface * 12, seed(\"g\"), 5) + cellular(surface * 30, seed(\"c\")) * 0.5;
let lift = fbm(surface * 40, seed(\"h\"), 4);
output base_color = vec3(clamp(heavy * tint, 0, 1) * occlusion);
output roughness = clamp(heavy * 0.5 + (1 - occlusion) * 0.3, 0, 1);
output metalness = 0;
output height = 0.002 * lift * external(\"lift\", 1.0);";
    for resolution in [256, 512] {
        let mut request = request(heavy.into(), resolution);
        let atlas = swatch_atlas(&request);
        let mut assets = Assets::default();
        assets.plane("occlusion.plane", &atlas, |texel| {
            (texel % 1009) as f32 / 1009.
        });
        assets.plane("lift.plane", &atlas, |texel| (texel % 499) as f32 / 499.);
        bind(&mut request, &["occlusion", "lift"], ".plane");
        let plane_bytes: usize = assets.0.values().map(|bytes| bytes.len()).sum();
        for run in 0..3 {
            let start = std::time::Instant::now();
            let local = fresh(&request, &assets);
            let local_seconds = start.elapsed().as_secs_f64();
            // The planes alone: a cold digest of the bytes (a new allocation) and the decode.
            let copies: Vec<Arc<[u8]>> = assets
                .0
                .values()
                .map(|bytes| bytes.to_vec().into())
                .collect();
            let start = std::time::Instant::now();
            for bytes in &copies {
                std::hint::black_box(image_digest(bytes));
                let plane =
                    program::Plane::decode_external("occlusion", bytes, atlas.width, atlas.height);
                std::hint::black_box(plane.unwrap());
            }
            let plane_seconds = start.elapsed().as_secs_f64();
            let mut line = format!(
                "{{\"resolution\":{resolution},\"run\":{run},\"plane_bytes\":{plane_bytes},\"local_s\":{local_seconds:.4},\"planes_digest_decode_s\":{plane_seconds:.5}"
            );
            for shares in [4, 8] {
                let mut session = Session::default();
                session.distribute_bakes(true);
                let _ = session.build(&request, 1, &|name| assets.get(name), &|| false);
                let pending = session.take_pending_bake().expect("a pending bake");
                let mut slowest = 0f64;
                let mut total = 0f64;
                for index in 0..shares {
                    // A helper is another session that has only the bytes it was sent.
                    let sent = Assets(
                        assets
                            .0
                            .iter()
                            .map(|(name, bytes)| (name.clone(), bytes.to_vec().into()))
                            .collect(),
                    );
                    let start = std::time::Instant::now();
                    let share = Session::default()
                        .bake_share(&pending, index, shares, &|name| sent.get(name), &|| false)
                        .unwrap_or_else(|e| panic!("{e:?}"));
                    let seconds = start.elapsed().as_secs_f64();
                    slowest = slowest.max(seconds);
                    total += seconds;
                    session.provide_bake_share(&share).unwrap();
                }
                let start = std::time::Instant::now();
                let (merged, _) = build(&mut session, 2, &request, &assets);
                let merge = start.elapsed().as_secs_f64();
                assert_same(&merged, &local, "distributed");
                line += &format!(
                    ",\"shares{shares}_slowest_s\":{slowest:.4},\"shares{shares}_sum_s\":{total:.4},\"shares{shares}_merge_encode_s\":{merge:.4}"
                );
            }
            eprintln!("{line}}}");
        }
    }
}

#[test]
fn a_field_bake_reads_planes_and_keeps_fields_by_their_content() {
    let mut request = request(source(0.5, 0.25, 1.), 32);
    let atlas = swatch_atlas(&request);
    let mut assets = Assets::default();
    let level = |texel: usize| (texel % 61) as f32 / 64.;
    assets.plane("shade.plane", &atlas, level);
    bind(&mut request, &["shade"], ".plane");
    let mut session = Session::default();
    let bake = |session: &mut Session, revision: u64, request: &Request, assets: &Assets| {
        FIELDS_BAKED.with(|baked| baked.borrow_mut().clear());
        let names = ["shade".to_owned(), "grain".to_owned()];
        let baked = session
            .bake_fields(request, revision, &names, &|name| assets.get(name), &|| {
                false
            })
            .unwrap_or_else(|e| panic!("{e:?}"));
        let evaluated: usize = FIELDS_BAKED.with(|baked| baked.borrow().iter().sum());
        (baked, evaluated)
    };
    let (first, evaluated) = bake(&mut session, 1, &request, &assets);
    assert_eq!(evaluated, 2);
    let mut own = 0;
    for chart in &atlas.charts {
        for y in 0..chart.height {
            for x in 0..chart.width {
                let texel = (chart.y + y) * atlas.width + chart.x + x;
                let value = first.fields[0][texel * 4];
                if value == level(texel) {
                    own += 1;
                } else {
                    assert_eq!(value, first.fields[0][(texel - 1) * 4], "texel {x},{y}");
                }
            }
        }
    }
    assert!(own >= 32 * 16, "{own} texels read their own value");
    let (again, evaluated) = bake(&mut session, 2, &request, &assets);
    assert_eq!(evaluated, 0, "both fields were kept");
    assert_eq!(again.fields, first.fields);
    // Another plane changes the field that reads it, and only that one.
    assets.plane("shade.other", &atlas, |texel| 1. - level(texel));
    let mut edited = request.clone();
    bind(&mut edited, &["shade"], ".other");
    let (changed, evaluated) = bake(&mut session, 3, &edited, &assets);
    assert_eq!(evaluated, 1, "only the field over the plane");
    assert_ne!(changed.fields[0], first.fields[0]);
    assert_eq!(changed.fields[1], first.fields[1]);
    let errors = session
        .bake_fields(
            &edited,
            4,
            &["shade".to_owned()],
            &|name| Assets::default().get(name),
            &|| false,
        )
        .err()
        .expect("a field bake without its plane fails");
    assert!(errors[0].message.contains("shade.other"), "{errors:?}");
}
