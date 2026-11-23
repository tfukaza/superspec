use superspec_aml::architecture::Value;
use superspec_profiles::{parse, serialize};

const COURTYARD: &str = r#"<architecture version="1" id="court">
  <city>
    <building id="ring" position="3 4 5" ground-shops="false" pavement="false"
              footprint="0.0004 0; 16 0; 16 14; 0 14">
      <hole points="4 4; 4 10; 12 10; 12 4"/>
    </building>
  </city>
</architecture>"#;

#[test]
fn city_courtyard_admits_and_round_trips_as_local_plan_geometry() {
    let source = parse(COURTYARD).unwrap();
    let round_trip = parse(&serialize(&source).unwrap()).unwrap();
    for document in [source, round_trip] {
        let building = &document.metadata[0].children[0];
        assert_eq!(building.id, "ring");
        assert_eq!(building.vector("position", [0.; 3]), [3., 4., 5.]);
        assert!(!building.boolean("ground-shops", true));
        assert!(!building.boolean("pavement", true));
        let Some(Value::PlanPoints(outer)) = building.attributes.get("footprint") else {
            panic!("missing building footprint")
        };
        assert_eq!(outer, &[[0., 0.], [16., 0.], [16., 14.], [0., 14.]]);
        let [hole] = building.children.as_slice() else {
            panic!("expected a single courtyard hole")
        };
        let Some(Value::PlanPoints(points)) = hole.attributes.get("points") else {
            panic!("missing hole points")
        };
        assert_eq!(points, &[[4., 4.], [4., 10.], [12., 10.], [12., 4.]]);
    }
}

#[test]
fn a_building_admits_graphics_detail_zero_to_two() {
    let at = |level: &str| {
        COURTYARD.replace(
            "ground-shops=",
            &format!("graphics-detail=\"{level}\" ground-shops="),
        )
    };
    for level in 0..=2 {
        let source = parse(&at(&level.to_string())).unwrap();
        let round_trip = parse(&serialize(&source).unwrap()).unwrap();
        for document in [source, round_trip] {
            let building = &document.metadata[0].children[0];
            assert_eq!(building.number("graphics-detail", 9.), f64::from(level));
        }
    }
    for level in ["3", "-1", "1.5", "low", ""] {
        assert_eq!(parse(&at(level)).unwrap_err().code, "AML_CITY", "{level}");
    }
}

#[test]
fn a_building_admits_a_variant_budget_up_to_the_largest_catalogue() {
    let at = |budget: &str| {
        COURTYARD.replace(
            "ground-shops=",
            &format!("variant-budget=\"{budget}\" ground-shops="),
        )
    };
    for budget in [0, 1, 2, 4, 5, 16, 64] {
        let source = parse(&at(&budget.to_string())).unwrap();
        let round_trip = parse(&serialize(&source).unwrap()).unwrap();
        for document in [source, round_trip] {
            let building = &document.metadata[0].children[0];
            assert_eq!(building.number("variant-budget", -9.), f64::from(budget));
        }
    }
    // Unstated, the budget is left to the graphics detail.
    let unstated = &parse(COURTYARD).unwrap().metadata[0].children[0];
    assert!(!unstated.attributes.contains_key("variant-budget"));
    for budget in ["65", "-1", "1.5", "few", ""] {
        assert_eq!(parse(&at(budget)).unwrap_err().code, "AML_CITY", "{budget}");
    }
}

#[test]
fn invalid_courtyard_syntax_and_conflicting_ground_trade_locate_the_source() {
    let missing_points = COURTYARD.replace(" points=\"4 4; 4 10; 12 10; 12 4\"", "");
    let invalid = parse(&missing_points).unwrap_err();
    assert_eq!(invalid.code, "AML_CITY");
    assert_eq!(invalid.source.line, 5);

    let front_trade = COURTYARD.replace("ground-shops=", "front-trade=\"roast\" ground-shops=");
    let invalid = parse(&front_trade).unwrap_err();
    assert_eq!(invalid.code, "AML_CITY");
    assert_eq!(invalid.objects, ["ring"]);
}
