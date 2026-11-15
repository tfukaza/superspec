use super::{SeededRng, raster::*};

pub(super) const COUNT: usize = 32;
const SIDE: f64 = 256.;
const WHITE: Rgb = [240, 238, 232];
const NAVY: Rgb = [62, 72, 98];
const RED: Rgb = [150, 84, 78];
const GREEN: Rgb = [88, 112, 98];
const GRAY: Rgb = [104, 106, 108];
const BLUE: Rgb = [98, 122, 152];
const BLOOMS: [Rgb; 4] = [
    [176, 108, 110],
    [192, 158, 104],
    [132, 136, 168],
    [196, 150, 158],
];
const CLOTH_SEAM: Rgb = [40, 36, 32];

fn fabric(color: Rgb, rng: &mut SeededRng, weave: f64, wrinkle: usize) -> Canvas {
    let mut image = Canvas::new(1, color);
    let creases =
        std::array::from_fn::<_, 20, _>(|_| 128. + normal(rng, wrinkle as f64 * 0.6).trunc());
    image.shade(&grain(image.size, weave, rng));
    image.shade(&smooth(
        image.size,
        [5; 2],
        128 - wrinkle,
        128 + wrinkle / 2,
        rng,
    ));
    image.shade(&interpolate(image.size, [20, 1], &creases));
    image
}

fn stitch(image: &mut Canvas, points: &[Point], strength: u8) {
    image.line(points, ink(CLOTH_SEAM, strength), 1.);
    let bright: Vec<_> = points.iter().map(|[x, y]| [x + 1., y + 1.]).collect();
    image.line(&bright, ink([255; 3], strength / 2), 1.);
}

fn stripes(image: &mut Canvas, color: Ink, pitch: usize, width: usize, horizontal: bool) {
    for start in (0..image.size).step_by(pitch) {
        let end = (start + width - 1) as f64;
        let start = start as f64;
        let bounds = if horizontal {
            [0., start, SIDE, end]
        } else {
            [start, 0., end, SIDE]
        };
        image.fill(bounds, color);
    }
}

fn checks(image: &mut Canvas, color: Ink, pitch: usize, width: usize) {
    stripes(image, color, pitch, width, true);
    stripes(image, color, pitch, width, false);
}

fn flowers(image: &mut Canvas, rng: &mut SeededRng, palette: &[Rgb], count: usize, size: f64) {
    use std::f64::consts::TAU;
    for _ in 0..count {
        let (x, y) = (rng.float(0., SIDE), rng.float(0., SIDE));
        let radius = size * rng.float(0.7, 1.3);
        let petal = rng.pick(palette);
        for _ in 0..2 {
            let angle = rng.float(0., TAU);
            let (lx, ly) = (
                x + angle.cos() * radius * 1.6,
                y + angle.sin() * radius * 1.6,
            );
            image.oval(
                ellipse(lx, ly, radius * 0.7, radius * 0.4),
                [112, 130, 104, 200],
            );
        }
        for k in 0..5 {
            let angle = f64::from(k) * TAU / 5. + rng.next_f64();
            let (cx, cy) = (
                x + angle.cos() * radius * 0.7,
                y + angle.sin() * radius * 0.7,
            );
            image.oval(ellipse(cx, cy, radius * 0.6, radius * 0.6), ink(petal, 230));
        }
        image.oval(
            ellipse(x, y, radius * 0.35, radius * 0.35),
            [214, 194, 128, 255],
        );
    }
}

#[derive(Clone, Copy)]
enum TopPattern {
    Plain,
    Hoops,
    Pinstripe,
    Check,
    Floral(usize),
    Rib,
}

#[derive(Clone, Copy)]
struct Top {
    base: Rgb,
    sleeve: f64,
    cuff: bool,
    collar: bool,
    placket: f64,
    scoop: f64,
    neck: f64,
    pockets: &'static [f64],
    pattern: TopPattern,
    motif: Option<bool>,
}

const TEE: Top = Top {
    base: WHITE,
    sleeve: 0.32,
    cuff: false,
    collar: false,
    placket: 0.,
    scoop: 0.085,
    neck: 0.115,
    pockets: &[],
    pattern: TopPattern::Plain,
    motif: None,
};

const SHIRT: Top = Top {
    sleeve: 0.96,
    cuff: true,
    collar: true,
    placket: 0.9,
    scoop: 0.07,
    neck: 0.1,
    ..TEE
};

const TOPS: [(Top, bool); 16] = [
    (TEE, false),
    (TEE, true),
    (
        Top {
            pattern: TopPattern::Hoops,
            ..TEE
        },
        false,
    ),
    (
        Top {
            motif: Some(true),
            ..TEE
        },
        false,
    ),
    (
        Top {
            sleeve: 0.,
            pattern: TopPattern::Rib,
            scoop: 0.2,
            neck: 0.15,
            ..TEE
        },
        false,
    ),
    (
        Top {
            collar: true,
            placket: 0.22,
            pattern: TopPattern::Rib,
            ..TEE
        },
        false,
    ),
    (
        Top {
            motif: Some(false),
            base: [214, 204, 170],
            ..TEE
        },
        false,
    ),
    (
        Top {
            pattern: TopPattern::Floral(0),
            base: [236, 228, 214],
            ..TEE
        },
        false,
    ),
    (
        Top {
            pockets: &[1.],
            ..SHIRT
        },
        false,
    ),
    (SHIRT, true),
    (
        Top {
            pattern: TopPattern::Pinstripe,
            pockets: &[1.],
            ..SHIRT
        },
        false,
    ),
    (
        Top {
            pattern: TopPattern::Pinstripe,
            ..SHIRT
        },
        true,
    ),
    (
        Top {
            pattern: TopPattern::Check,
            base: [232, 226, 214],
            pockets: &[-1., 1.],
            ..SHIRT
        },
        false,
    ),
    (
        Top {
            pattern: TopPattern::Check,
            base: [232, 226, 214],
            ..SHIRT
        },
        true,
    ),
    (
        Top {
            pattern: TopPattern::Rib,
            pockets: &[-1., 1.],
            ..SHIRT
        },
        false,
    ),
    (
        Top {
            sleeve: 0.3,
            pattern: TopPattern::Floral(1),
            base: [222, 226, 232],
            scoop: 0.11,
            neck: 0.13,
            ..TEE
        },
        false,
    ),
];

fn button(image: &mut Canvas, x: f64, y: f64, radius: f64, color: Ink, outline: u8) {
    image.oval_with_outline(
        ellipse(x, y, radius, radius),
        Some(color),
        Some([60, 60, 60, outline]),
        1.,
    );
}

fn top(design: Top, back: bool, rng: &mut SeededRng) -> Canvas {
    let mut image = fabric(design.base, rng, 3.5, 9);
    match design.pattern {
        TopPattern::Plain => (),
        TopPattern::Hoops => stripes(&mut image, ink(NAVY, 255), 26, 10, true),
        TopPattern::Pinstripe => stripes(&mut image, ink(BLUE, 190), 9, 2, false),
        TopPattern::Check => checks(&mut image, ink(RED, 110), 22, 9),
        TopPattern::Floral(start) => flowers(&mut image, rng, &BLOOMS[start..], 70, 6.),
        TopPattern::Rib => stripes(&mut image, [60, 60, 60, 38], 4, 1, false),
    }
    // The middle strip maps to the body. The two outer strips map to sleeves.
    let (left, right, middle) = (SIDE * 0.2, SIDE * 0.8, SIDE / 2.);
    let sleeve = design.sleeve * SIDE;
    for x in [left, right] {
        stitch(&mut image, &[[x, 0.], [x, SIDE * 0.4]], 70);
    }
    if sleeve > 0. {
        for (x0, x1) in [(0., left), (right, SIDE)] {
            if design.cuff {
                image.fill([x0, sleeve - 20., x1, sleeve], [0, 0, 0, 26]);
                stitch(&mut image, &[[x0, sleeve - 20.], [x1, sleeve - 20.]], 70);
                button(
                    &mut image,
                    (x0 + x1) / 2.,
                    sleeve - 10.,
                    2.,
                    [250, 250, 246, 255],
                    160,
                );
            } else {
                stitch(&mut image, &[[x0, sleeve - 8.], [x1, sleeve - 8.]], 70);
            }
        }
    }
    let depth = if back { 0.03 } else { design.scoop } * SIDE;
    let half = design.neck * SIDE;
    let opening = ellipse(middle, 0., half, depth);
    image.oval(opening, [0, 0, 0, 70]);
    if !back {
        image.fill([middle - 7., 3., middle + 7., 10.], [244, 242, 236, 255]);
    }
    match (design.collar, back) {
        (true, false) => {
            for side in [-1., 1.] {
                image.polygon(
                    &[
                        [middle + side * half, 0.],
                        [middle + side * (half + 12.), 0.],
                        [middle + side * (half + 4.), depth + 22.],
                        [middle + side * 3., depth + 4.],
                    ],
                    Some([0, 0, 0, 34]),
                    Some(ink(CLOTH_SEAM, 120)),
                );
            }
        }
        (true, true) => {
            let bounds = [middle - half - 10., 0., middle + half + 10., depth + 6.];
            image.fill(bounds, [0, 0, 0, 30]);
            image.frame(bounds, ink(CLOTH_SEAM, 110), 1.);
            stitch(
                &mut image,
                &[
                    [left, SIDE * 0.17],
                    [middle, SIDE * 0.2],
                    [right, SIDE * 0.17],
                ],
                70,
            );
        }
        (false, _) => {
            image.arc(
                ellipse(middle, 0., half + 5., depth + 5.),
                0.,
                180.,
                ink(CLOTH_SEAM, 90),
                1.,
            );
            image.arc(opening, 0., 180., ink(CLOTH_SEAM, 130), 2.);
        }
    }
    if !back {
        if design.placket > 0. {
            let end = depth + design.placket * SIDE;
            image.fill([middle - 7., depth, middle + 7., end], [0, 0, 0, 16]);
            for x in [middle - 7., middle + 7.] {
                stitch(&mut image, &[[x, depth], [x, end]], 70);
            }
            for y in (depth as usize + 14..end as usize - 4).step_by(30) {
                button(&mut image, middle, y as f64, 3., [250, 250, 246, 255], 170);
            }
        }
        for side in design.pockets {
            let x = middle + side * (right - left) * 0.25;
            let y = SIDE * 0.27;
            image.frame([x - 22., y, x + 22., y + 46.], ink(CLOTH_SEAM, 120), 1.);
            stitch(&mut image, &[[x - 22., y + 8.], [x + 22., y + 8.]], 70);
        }
        if let Some(block) = design.motif {
            let (x0, y0, x1, y1) = (middle - 46., SIDE * 0.3, middle + 46., SIDE * 0.3 + 78.);
            if block {
                image.rounded([x0, y0, x1, y1], 6., Some(ink(RED, 210)), None, 1.);
                for offset in [0., 1., 2.] {
                    image.fill(
                        [
                            x0 + 12.,
                            y0 + 16. + offset * 20.,
                            x1 - 12. - offset * 14.,
                            y0 + 24. + offset * 20.,
                        ],
                        [244, 240, 228, 230],
                    );
                }
            } else {
                image.oval([x0 + 6., y0, x1 - 6., y1 - 4.], ink(BLUE, 215));
                image.oval(
                    [x0 + 24., y0 + 16., x1 - 24., y1 - 22.],
                    [246, 222, 120, 235],
                );
            }
        }
    }
    stitch(&mut image, &[[0., SIDE - 9.], [SIDE, SIDE - 9.]], 70);
    stitch(&mut image, &[[0., SIDE - 5.], [SIDE, SIDE - 5.]], 40);
    image
}

fn bottom(index: usize, rng: &mut SeededRng) -> Canvas {
    let (base, band, rise) = match index {
        18 | 19 => ([84, 104, 138], 0.07, 0.3),
        20 => (WHITE, 0.12, 0.6),
        21 => ([232, 234, 238], 0.06, 0.3),
        22 => ([226, 230, 236], 0.14, 0.7),
        _ => (WHITE, 0.07, 0.3),
    };
    let mut image = fabric(base, rng, 3.5, 9);
    let (back, elastic, twill) = (
        matches!(index, 17 | 19),
        index >= 20,
        matches!(index, 18 | 19),
    );
    if twill {
        for offset in (-256..256).step_by(4) {
            let x = f64::from(offset);
            image.line(&[[x, SIDE], [x + SIDE, 0.]], [255, 255, 255, 26], 1.);
        }
        let mut fade = Canvas::new(1, [0; 3]);
        for x in [SIDE * 0.27, SIDE * 0.73] {
            fade.oval(
                [x - 26., SIDE * 0.22, x + 26., SIDE * 0.86],
                [70, 70, 70, 255],
            );
        }
        image.glaze([176, 190, 206], blur(&fade.mask(), fade.size, 16.));
    } else if index == 21 {
        stripes(&mut image, ink(BLUE, 170), 12, 3, false);
    } else if index == 22 {
        checks(&mut image, ink(NAVY, 110), 20, 8);
    }
    let (middle, band, crotch) = (SIDE / 2., band * SIDE, rise * SIDE);
    image.fill([0., 0., SIDE, band], [0, 0, 0, 22]);
    stitch(&mut image, &[[0., band], [SIDE, band]], 70);
    if elastic {
        for x in (4..256).step_by(7) {
            let x = f64::from(x);
            image.line(&[[x, 2.], [x, band - 2.]], ink(CLOTH_SEAM, 50), 1.);
        }
        for side in [-1., 1.] {
            image.line(
                &[
                    [middle + side * 5., band - 2.],
                    [middle + side * 12., band + 34.],
                ],
                [236, 232, 220, 255],
                2.,
            );
        }
    } else {
        for x in [0.1, 0.36, 0.64, 0.9].map(|x| x * SIDE) {
            let bounds = [x - 3., 1., x + 3., band + 2.];
            image.fill(bounds, [0, 0, 0, 34]);
            image.frame(bounds, ink(CLOTH_SEAM, 110), 1.);
        }
        if !back {
            button(&mut image, middle, band / 2., 4., [196, 190, 170, 255], 200);
        }
    }
    stitch(&mut image, &[[middle, band], [middle, SIDE]], 90);
    match (back, elastic) {
        (false, false) => {
            image.arc(
                [middle - 18., band - 30., middle + 18., crotch],
                300.,
                90.,
                ink(CLOTH_SEAM, 120),
                1.,
            );
            image.arc(
                ellipse(SIDE, band, 46., 52.),
                20.,
                80.,
                ink(CLOTH_SEAM, 140),
                2.,
            );
            image.arc(
                ellipse(0., band, 46., 52.),
                100.,
                160.,
                ink(CLOTH_SEAM, 140),
                2.,
            );
        }
        (true, false) => {
            for x in [SIDE * 0.25, SIDE * 0.75] {
                if twill {
                    image.polygon(
                        &[
                            [x - 26., band + 22.],
                            [x + 26., band + 22.],
                            [x + 26., band + 66.],
                            [x, band + 78.],
                            [x - 26., band + 66.],
                        ],
                        None,
                        Some([226, 170, 96, 220]),
                    );
                } else {
                    image.fill(
                        [x - 24., band + 26., x + 24., band + 29.],
                        ink(CLOTH_SEAM, 120),
                    );
                }
            }
            stitch(
                &mut image,
                &[[0., band + 14.], [middle, band + 22.], [SIDE, band + 14.]],
                70,
            );
        }
        (_, true) => (),
    }
    if index <= 17 {
        for x in [SIDE * 0.25, SIDE * 0.75] {
            image.line(&[[x, crotch * 0.7], [x, SIDE]], [255, 255, 255, 60], 1.);
        }
    }
    stitch(&mut image, &[[0., SIDE - 10.], [SIDE, SIDE - 10.]], 70);
    image
}

fn flat_good(index: usize, rng: &mut SeededRng) -> Canvas {
    let (base, weave, wrinkle) = match index {
        23 => ([236, 232, 222], 3.5, 9),
        26 => ([226, 232, 226], 3.5, 9),
        27 => ([238, 230, 224], 3.5, 9),
        28 => (WHITE, 3.5, 14),
        29 => ([214, 200, 168], 6., 9),
        30 => ([226, 190, 180], 3.5, 5),
        31 => (WHITE, 3.5, 16),
        _ => (WHITE, 3.5, 9),
    };
    let mut image = fabric(base, rng, weave, wrinkle);
    match index {
        23 => {
            checks(&mut image, ink(RED, 120), 32, 12);
            checks(&mut image, ink(GRAY, 150), 64, 3);
        }
        24 => {
            for y in [22., SIDE - 34.] {
                image.fill([0., y, SIDE, y + 12.], ink(BLUE, 255));
                image.fill([0., y + 16., SIDE, y + 18.], ink(BLUE, 200));
                image.fill([0., y - 5., SIDE, y - 4.], [0, 0, 0, 40]);
            }
        }
        25 => {
            let colors = [[178, 122, 114], [206, 188, 138], [126, 152, 164]];
            for (index, x) in (0..256).step_by(32).enumerate() {
                let x = f64::from(x);
                image.fill(
                    [x, 0., x + 15., SIDE],
                    ink(colors[index % colors.len()], 235),
                );
            }
        }
        26 => flowers(&mut image, rng, &BLOOMS[..2], 30, 9.),
        27 => flowers(&mut image, rng, &BLOOMS, 46, 9.),
        28 => {
            image.frame([14., 14., SIDE - 15., SIDE - 15.], ink(BLUE, 230), 5.);
            image.frame([26., 26., SIDE - 27., SIDE - 27.], ink(BLUE, 150), 1.);
        }
        29 => {
            checks(&mut image, ink(GREEN, 120), 64, 26);
            checks(&mut image, ink(NAVY, 150), 64, 6);
            checks(&mut image, ink(RED, 170), 128, 3);
        }
        30 => {
            for offset in (-256..512).step_by(36) {
                let x = f64::from(offset);
                stitch(&mut image, &[[x, 0.], [x + SIDE, SIDE]], 60);
                stitch(&mut image, &[[x, SIDE], [x + SIDE, 0.]], 60);
            }
        }
        31 => {
            let mut marks = Canvas::new(1, [0; 3]);
            for _ in 0..7 {
                let (x, y, radius) = (
                    rng.float(0., SIDE),
                    rng.float(0., SIDE),
                    rng.float(10., 40.),
                );
                marks.oval(
                    ellipse(x, y, radius, radius * 0.7),
                    ink([rng.integer(30, 80) as u8; 3], 255),
                );
            }
            image.glaze([150, 132, 104], blur(&marks.mask(), marks.size, 8.));
        }
        _ => unreachable!("flat cloth atlas index"),
    }
    if (24..=26).contains(&index) {
        image.shade(&blur(&grain(image.size, 9., rng), image.size, 0.7));
    }
    for y in [4., SIDE - 6.] {
        stitch(&mut image, &[[0., y], [SIDE, y]], 60);
    }
    image
}

pub(super) fn tile(index: usize, rng: &mut SeededRng) -> Canvas {
    match index {
        0..=15 => {
            let (design, back) = TOPS[index];
            top(design, back, rng)
        }
        16..=22 => bottom(index, rng),
        _ => flat_good(index, rng),
    }
}
