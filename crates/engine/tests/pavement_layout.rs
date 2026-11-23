use superspec_engine::worldspec::themes::kowloon::pavement::{
    context::{self, Wall},
    field, layout,
};
use superspec_geometry::obstructions::Budget;

#[test]
fn convex_cuts_partition_receiving_area_and_keep_heights() {
    let receiving = field::rectangle(0., 0., 4., 3., 0.25);
    let tilted = vec![[1., -1., 0.], [5., 1., 0.], [3., 4., 0.], [-0.5, 2., 0.]];
    let outside = field::rectangle(10., 10., 1., 1., 0.);
    for boundary in [field::rectangle(1., 1., 2., 1., 0.), tilted, outside] {
        let split = field::split(&receiving, &boundary);
        let partitioned = field::area(&split.inside).abs()
            + split
                .outside
                .iter()
                .map(|p| field::area(p).abs())
                .sum::<f64>();
        assert!(
            (partitioned - field::area(&receiving).abs()).abs() < 1e-8,
            "partition preserves receiving area"
        );
        let inside = field::intersect(&receiving, &boundary);
        assert!((field::area(&inside).abs() - field::area(&split.inside).abs()).abs() < 1e-8);
        assert!(
            split
                .outside
                .iter()
                .flatten()
                .chain(&split.inside)
                .all(|p| p[2] == 0.25),
            "cuts retain the receiving height"
        );
    }
    assert_eq!(field::area(&receiving), 12.);
    assert_eq!(
        field::edge_distance([2., 5., 0.], [0., 0., 0.], [4., 0., 0.]),
        5.
    );
    assert_eq!(
        field::edge_distance([7., 4., 0.], [0., 0., 0.], [4., 0., 0.]),
        5.
    );
}
#[test]
fn opposing_walls_from_distinct_hosts_establish_corridor_width() {
    let mut budget = Budget::new(100_000, &|| false).unwrap();
    let wall = |host: &str, y: f64| Wall {
        host: host.into(),
        seam: [[-5., y, 0.], [5., y, 0.]],
    };
    let open = context::exposure([0., 0., 0.], &[], &mut budget).unwrap();
    assert!(open.wall_distance.is_infinite() && open.corridor_width.is_infinite());
    let lane =
        context::exposure([0., 0.5, 0.], &[wall("a", 0.), wall("b", 2.)], &mut budget).unwrap();
    assert!((lane.wall_distance - 0.5).abs() < 1e-12);
    assert!((lane.corridor_width - 2.).abs() < 1e-12);
    let same =
        context::exposure([0., 0.5, 0.], &[wall("a", 0.), wall("a", 2.)], &mut budget).unwrap();
    assert!(
        same.corridor_width.is_infinite(),
        "one host is not a corridor"
    );
}
#[test]
fn admission_and_work_limits_fail_without_poisoning_recovery() {
    let loops = vec![field::rectangle(0., 0., 8., 6., 0.)];
    let mut budget = Budget::new(1_000_000, &|| false).unwrap();
    assert!(layout::footprint(&[], 1.9, 7., &mut budget).is_err());
    assert!(layout::footprint(&loops, 0., 7., &mut budget).is_err());
    assert!(layout::footprint(&loops, 1.9, f64::NAN, &mut budget).is_err());
    assert!(layout::footprint(&loops, 1.9, 7., &mut Budget::new(1, &|| false).unwrap()).is_err());
    assert!(layout::footprint(&loops, 1.9, 7., &mut Budget::new(1000, &|| true).unwrap()).is_err());
    assert!(context::exposure([0.; 3], &[], &mut Budget::new(1000, &|| true).unwrap()).is_err());
    let oversized = vec![field::rectangle(0., 0., 20_000., 3., 0.)];
    assert!(layout::footprint(&oversized, 0.5, 7., &mut budget).is_err());
    let l = layout::footprint(&loops, 1.9, 7., &mut budget).unwrap();
    assert!(layout::pours(&l, 9, 1.1, &mut budget).is_err());
    assert!(layout::pours(&l, 9, 0.5, &mut Budget::new(1000, &|| true).unwrap()).is_err());
    let pours = layout::pours(&l, 9, 1., &mut budget).unwrap();
    let mut cells = vec![0; (l.columns.len() - 1) * (l.rows.len() - 1)];
    for p in pours {
        for x in p.x..p.end_x {
            for y in p.y..p.end_y {
                cells[x * (l.rows.len() - 1) + y] += 1;
            }
        }
    }
    assert!(
        cells.into_iter().all(|n| n == 1),
        "joined pours cover every original cell exactly once"
    );
}
