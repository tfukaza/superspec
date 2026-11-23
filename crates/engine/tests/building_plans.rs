use superspec_engine::worldspec::themes::kowloon::massing::{
    Rect, SiteBudget, default_stories, lot_blocks, plan_lots, subtract_rect,
};

#[test]
fn seeded_building_plans_are_repeatable_and_vary_kind_awning_and_frontage() {
    let mut kinds = std::collections::BTreeSet::new();
    let mut awnings = std::collections::BTreeSet::new();
    let mut fronts = std::collections::BTreeSet::new();
    for (seed, site) in [
        ("court-01/background-west", (8.11, 5., 11.8, 4, 3.)),
        ("court-01/background-east", (6., 5., 14.6, 5, 0.)),
        ("court-01/shop-row", (30., 8., 15.35, 5, 0.)),
        ("single", (3., 3., 3.15, 1, 0.)),
        (
            "courtyard/\u{5ead}\u{9662}\u{1f33f}",
            (40., 14., 30.35, 10, 4.2),
        ),
        ("short-depth", (18., 3., 9.35, 3, 2.9)),
        ("branch-0", (30., 8., 15.35, 5, 0.)),
    ] {
        let (width, depth, height, stories, base_height) = site;
        let site = SiteBudget {
            width,
            depth,
            height,
            stories,
            base_height,
        };
        let lots = plan_lots(site, seed).unwrap();
        // The same seed and budget always plan the same lots and volumes.
        assert_eq!(
            serde_json::to_value(&lots).unwrap(),
            serde_json::to_value(plan_lots(site, seed).unwrap()).unwrap(),
            "{seed}"
        );
        let blocks: Vec<_> = lots.iter().flat_map(|l| lot_blocks(l, depth)).collect();
        assert!(!blocks.is_empty(), "{seed}");
        for lot in lots {
            kinds.insert(lot.kind);
            awnings.insert(lot.style.awning.canvas);
            fronts.insert(lot.front.total_cmp(&0.));
        }
    }
    // Both lot kinds, both awning materials and recessed, flush and projecting fronts occur.
    assert_eq!(kinds.len(), 2);
    assert_eq!(awnings.len(), 2);
    assert_eq!(fronts.len(), 3);
}
#[test]
fn lot_coverage_story_datums_and_body_volumes_obey_site_budgets() {
    for width in [3., 8.499, 8.5, 8.501, 30., 40.] {
        for depth in [3., 4.999, 5., 5.999, 6., 10.] {
            for stories in [1, 3, 5] {
                let site = SiteBudget {
                    width,
                    depth,
                    height: stories as f64 * 3. + 0.35,
                    stories,
                    base_height: 2.6,
                };
                for seed in ["a", "b", "c", "庭院🌿"] {
                    let lots = plan_lots(site, seed).unwrap();
                    let mut end = 0.;
                    let mut top = 0.0_f64;
                    for (index, lot) in lots.iter().enumerate() {
                        assert_eq!(lot.index, index);
                        assert!((lot.x0 - end).abs() < 1e-12 && lot.x1 > lot.x0);
                        assert!(lot.ground >= site.base_height && lot.top <= site.height + 1e-12);
                        assert!(
                            (lot.ground + (lot.stories - 1) as f64 * lot.story_height - lot.roof)
                                .abs()
                                < 1e-12
                        );
                        let blocks = lot_blocks(lot, depth);
                        assert_eq!(blocks[0].y0, 0.);
                        assert_eq!(blocks[0].y1, depth);
                        assert_eq!(blocks[0].z0, 0.);
                        let actual: f64 = blocks
                            .iter()
                            .map(|b| (b.x1 - b.x0) * (b.y1 - b.y0) * (b.z1 - b.z0))
                            .sum();
                        let expected = (lot.x1 - lot.x0)
                            * (depth * lot.ground
                                + if lot.top - lot.ground >= 0.05 {
                                    (depth + lot.front + lot.back) * (lot.top - lot.ground)
                                } else {
                                    0.
                                });
                        assert!((actual - expected).abs() < 1e-9);
                        if depth < 5. {
                            assert_ne!(lot.kind, "well");
                        }
                        if depth < 6. && lot.kind == "tenement" {
                            assert!(lot.front >= 0.);
                        }
                        end = lot.x1;
                        top = top.max(lot.top);
                    }
                    assert!((end - width).abs() < 1e-12 && (top - site.height).abs() < 1e-12);
                    let tenements: Vec<_> = lots.iter().filter(|l| l.kind == "tenement").collect();
                    if tenements.len() > 1 && stories >= 3 {
                        assert!(tenements.iter().any(|l| l.top < site.height - 0.1));
                    }
                }
            }
        }
    }
    assert_eq!(default_stories(9.349), 2);
    assert_eq!(default_stories(9.35), 3);
    let valid = SiteBudget {
        width: 10.,
        depth: 5.,
        height: 9.35,
        stories: 3,
        base_height: 0.,
    };
    for bad in [
        SiteBudget {
            width: f64::NAN,
            ..valid
        },
        SiteBudget {
            depth: f64::INFINITY,
            ..valid
        },
        SiteBudget {
            height: f64::NEG_INFINITY,
            ..valid
        },
        SiteBudget {
            base_height: f64::NAN,
            ..valid
        },
        SiteBudget {
            width: 2.999,
            ..valid
        },
        SiteBudget {
            width: 40.001,
            ..valid
        },
        SiteBudget {
            depth: 2.999,
            ..valid
        },
        SiteBudget {
            depth: 40.001,
            ..valid
        },
        SiteBudget {
            stories: 0,
            ..valid
        },
        SiteBudget {
            base_height: 9.1,
            ..valid
        },
    ] {
        assert!(plan_lots(bad, "invalid-budget").is_err(), "{bad:?}");
    }
    assert!(
        plan_lots(
            SiteBudget {
                width: 10.,
                depth: 5.,
                height: 2.,
                stories: 1,
                base_height: 0.
            },
            "bad"
        )
        .is_err()
    );
    assert!(
        plan_lots(
            SiteBudget {
                width: 10.,
                depth: 5.,
                height: 9.35,
                stories: usize::MAX,
                base_height: 0.
            },
            "bad"
        )
        .is_err()
    );
    let rect = Rect {
        u0: 0.,
        u1: 4.,
        v0: 0.,
        v1: 3.,
    };
    let pieces = subtract_rect(
        rect,
        Rect {
            u0: 1.,
            u1: 3.,
            v0: 1.,
            v1: 2.,
        },
        1e-6,
    );
    assert_eq!(pieces.len(), 4);
    assert_eq!(
        pieces
            .iter()
            .map(|r| (r.u1 - r.u0) * (r.v1 - r.v0))
            .sum::<f64>(),
        10.
    );
}
