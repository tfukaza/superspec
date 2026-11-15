use super::{SeededRng, raster::*};
use std::f64::consts::TAU;

pub(super) const COUNT: usize = 26;
const SIDE: f64 = 256.;
const PANEL: usize = 512;
const PAINT: Rgb = [238, 238, 234];
const RUST: Rgb = [142, 80, 42];
const CREAM: Rgb = [236, 228, 204];
const GRAY: Rgb = [200, 204, 204];

fn weathered(mut image: Canvas, rng: &mut SeededRng, wear: f64, rusting: bool) -> Canvas {
    for _ in 0..(3. + wear * 18.) as usize {
        let (x, y) = (rng.float(0., SIDE), rng.float(0., SIDE / 2.));
        image.fill(
            [x, y, x + rng.float(0.5, 2.5), y + rng.float(30., SIDE)],
            [70, 60, 48, (10. + wear * 26.) as u8],
        );
    }
    for y in 0..PANEL {
        let depth = ((y as f64 / PANEL as f64 - (0.86 - wear * 0.22)) / 0.3).clamp(0., 1.);
        let y = y as f64 * 0.5;
        image.fill(
            [0., y, SIDE, y],
            [84, 74, 60, (depth * (30. + wear * 100.)) as u8],
        );
    }
    if rusting && wear > 0.3 {
        let broad = smooth(PANEL, [5; 2], 0, 255, rng);
        let fine = smooth(PANEL, [34; 2], 60, 255, rng);
        let threshold = 118. - wear * 62.;
        let rust: Vec<_> = broad
            .into_iter()
            .zip(fine)
            .enumerate()
            .map(|(index, (a, b))| {
                let t = (index / PANEL) as f64 / PANEL as f64;
                let edge = (0.12 + 0.9 * t.powi(4) + 0.35 * (1. - t * 9.).max(0.)).min(1.);
                let value = f64::from(u16::from(a) * u16::from(b) / 255);
                ((value * edge - threshold) * 9.) as u8
            })
            .collect();
        let mut runoff = vec![0; rust.len()];
        for step in (4..80).step_by(6) {
            let factor = 0.42 * (1. - step as f64 / 84.);
            for (dst, src) in runoff[step * PANEL..].iter_mut().zip(&rust) {
                *dst = (*dst).max((f64::from(*src) * factor) as u8);
            }
        }
        image.glaze(tinted(RUST, 1.15), blur(&runoff, PANEL, 2.));
        image.glaze(RUST, blur(&rust, PANEL, 1.6));
        let pits = rust.into_iter().zip(grain(PANEL, 46., rng)).map(|(a, b)| {
            if u16::from(a) * u16::from(b) / 255 > 150 {
                255
            } else {
                0
            }
        });
        image.glaze(tinted(RUST, 0.55), pits);
    }
    image.blur(0.5);
    image
}

fn sheet_metal(rng: &mut SeededRng, wear: f64, base: Rgb) -> Canvas {
    let mut image = Canvas::new(2, base);
    image.shade(&smooth(PANEL, [6; 2], 116, 130, rng));
    image.shade(&smooth(PANEL, [40; 2], 124, 131, rng));
    image.shade(&grain(PANEL, 2.2, rng));
    for _ in 0..(6. + wear * 30.) as usize {
        let (x, y, length) = (rng.float(0., SIDE), rng.float(0., SIDE), rng.float(6., 40.));
        image.line(
            &[[x, y], [x + length, y + rng.float(-10., 10.)]],
            [90, 90, 86, 34],
            1.,
        );
    }
    weathered(image, rng, wear, true)
}

fn screw(image: &mut Canvas, x: f64, y: f64, rusty: bool) {
    if rusty {
        image.fill([x - 1., y, x + 1., y + 22.], ink(RUST, 110));
    }
    image.oval_with_outline(
        ellipse(x, y, 4., 4.),
        Some([150, 150, 146, 255]),
        Some([70, 70, 68, 255]),
        1.,
    );
    image.line(&[[x - 3., y], [x + 3., y]], [60, 60, 58, 255], 1.);
    image.oval([x - 2.2, y - 2.8, x - 0.6, y - 1.6], [232, 232, 228, 255]);
}

fn label(image: &mut Canvas, x: f64, y: f64, width: f64, height: f64, rng: &mut SeededRng) {
    let bounds = [x, y, x + width, y + height];
    image.fill(bounds, [226, 224, 214, 255]);
    image.frame(bounds, [120, 118, 110, 255], 1.);
    let mut row = height * 0.36;
    while row < height - 3. {
        image.fill(
            [
                x + 3.,
                y + row,
                x + width - rng.float(4., width * 0.5),
                y + row + 1.,
            ],
            [70, 70, 70, 200],
        );
        row += 3.5;
    }
}

fn side_panel(rng: &mut SeededRng, wear: f64) -> Canvas {
    let mut image = sheet_metal(rng, wear, PAINT);
    image.frame([6., 6., SIDE - 7., SIDE - 7.], [60, 60, 58, 70], 1.);
    image.line(&[[6., 7.5], [SIDE - 7., 7.5]], [255, 255, 255, 90], 1.);
    for x in [18., SIDE - 18.] {
        for y in [18., SIDE - 18.] {
            screw(&mut image, x, y, wear > 0.4);
        }
    }
    if wear < 0.5 {
        label(&mut image, SIDE - 76., SIDE - 62., 52., 38., rng);
    }
    image
}

fn top_panel(rng: &mut SeededRng, wear: f64) -> Canvas {
    let mut image = sheet_metal(rng, wear * 0.7, PAINT);
    let dust = smooth(PANEL, [7; 2], 0, (60. + wear * 120.) as usize, rng);
    image.glaze([128, 118, 100], dust.into_iter().map(|v| v / 2));
    image.frame([5., 5., SIDE - 6., SIDE - 6.], [60, 60, 58, 60], 1.);
    if wear > 0.5 {
        for _ in 0..9 {
            let x = rng.float(10., SIDE - 10.);
            let y = rng.float(10., SIDE - 10.);
            let radius = rng.float(3., 9.);
            image.oval(ellipse(x, y, radius, radius * 0.8), [236, 236, 228, 220]);
            image.oval(
                [
                    x - radius * 0.3,
                    y - radius * 0.3,
                    x + radius * 0.4,
                    y + radius * 0.3,
                ],
                [70, 74, 66, 200],
            );
        }
        for _ in 0..12 {
            image.oval(
                ellipse(rng.float(0., SIDE), rng.float(0., SIDE), 7., 3.),
                [104, 84, 48, 220],
            );
        }
        let (x, y) = (rng.float(70., SIDE - 70.), rng.float(70., SIDE - 70.));
        image.oval_with_outline(ellipse(x, y, 30., 30.), None, Some(ink(RUST, 200)), 3.);
    }
    image
}

fn fins(rng: &mut SeededRng, wear: f64) -> Canvas {
    let mut image = Canvas::new(2, [46, 50, 52]);
    for y in (10..256).step_by(22) {
        let y = f64::from(y);
        image.fill([0., y, SIDE, y + 5.], [110, 92, 70, 120]);
    }
    for x in (0..256).step_by(2) {
        let x = f64::from(x);
        let tone = rng.integer(168, 204) as u8;
        image.fill([x, 0., x, SIDE], [tone, tone + 5, tone + 7, 235]);
        image.fill(
            [x + 0.5, 0., x + 0.5, SIDE],
            [tone - 60, tone - 56, tone - 54, 200],
        );
    }
    let mut dents = Canvas::new(2, [0; 3]);
    for _ in 0..(4. + wear * 14.) as usize {
        let (x, y) = (rng.float(0., SIDE), rng.float(0., SIDE));
        let (rx, ry) = (rng.float(5., 22.), rng.float(3., 12.));
        dents.oval(
            ellipse(x, y, rx, ry),
            ink([rng.integer(120, 230) as u8; 3], 255),
        );
    }
    image.glaze([128, 132, 134], blur(&dents.mask(), PANEL, 3.));
    let clog = smooth(PANEL, [6; 2], 0, (40. + wear * 200.) as usize, rng);
    image.glaze([58, 52, 44], clog.into_iter().map(|v| v.saturating_sub(60)));
    image
}

fn mesh(rng: &mut SeededRng, wear: f64) -> Canvas {
    let mut image = fins(rng, wear);
    for pixel in &mut image.pixels {
        for channel in &mut pixel[..3] {
            *channel /= 2;
        }
    }
    for offset in (-256..512).step_by(12) {
        let start = f64::from(offset);
        let end = start + SIDE / 2.;
        let down = [[start, 0.], [end, SIDE]];
        let up = [[start, SIDE], [end, 0.]];
        image.line(&down, [60, 60, 58, 255], 3.);
        image.line(&up, [60, 60, 58, 255], 3.);
        image.line(&down, [216, 216, 210, 255], 2.);
        image.line(&up, [198, 198, 192, 255], 2.);
    }
    weathered(image, rng, wear * 0.8, true)
}

fn framed(mut image: Canvas, rng: &mut SeededRng, wear: f64) -> Canvas {
    let (rim, step) = (0.05 * SIDE, 0.075 * SIDE);
    for inset in 0..rim as u8 {
        let tone = 236 - inset;
        let d = f64::from(inset);
        image.frame([d, d, SIDE - d, SIDE - d], [tone, tone, tone - 4, 255], 1.);
    }
    image.frame(
        [rim, rim, SIDE - rim, SIDE - rim],
        [24, 24, 24, 235],
        (step - rim).floor(),
    );
    for x in [6., SIDE - 6.] {
        for y in [6., SIDE - 6.] {
            image.oval_with_outline(
                ellipse(x, y, 2.5, 2.5),
                Some([120, 120, 116, 255]),
                Some([60, 60, 58, 255]),
                1.,
            );
        }
    }
    weathered(image, rng, wear * 0.9, true)
}

#[derive(Clone, Copy)]
enum Rear {
    Louvers,
    Grid,
    Coil,
    Mesh,
}

fn rear(kind: Rear, rng: &mut SeededRng, wear: f64) -> Canvas {
    let image = match kind {
        Rear::Louvers => {
            let mut image = sheet_metal(rng, 0., PAINT);
            for y in (24..230).step_by(11) {
                let y = f64::from(y);
                for x0 in [24., SIDE / 2. + 4.] {
                    let x1 = x0 + SIDE / 2. - 28.;
                    image.fill([x0, y, x1, y + 5.], [22, 24, 26, 255]);
                    image.fill([x0, y + 5., x1, y + 6.5], [255, 255, 255, 170]);
                    image.fill([x0, y - 1.5, x1, y], [96, 96, 94, 170]);
                    image.fill([x0 - 1.5, y, x0, y + 5.], [110, 110, 108, 150]);
                }
            }
            image
        }
        Rear::Grid => {
            let mut image = fins(rng, wear);
            for x in (8..256).step_by(20) {
                let x = f64::from(x);
                image.fill([x, 0., x + 2., SIDE], [226, 226, 220, 255]);
                image.line(&[[x + 2.75, 0.], [x + 2.75, SIDE]], [20, 20, 20, 170], 1.);
            }
            for y in (8..256).step_by(40) {
                let y = f64::from(y);
                image.fill([0., y, SIDE, y + 3.], [232, 232, 226, 255]);
                image.line(&[[0., y + 3.75], [SIDE, y + 3.75]], [20, 20, 20, 170], 1.);
            }
            image
        }
        Rear::Coil => {
            let mut image = fins(rng, wear * 1.3);
            for y in [SIDE * 0.3, SIDE * 0.7] {
                let bounds = [0., y - 5., SIDE, y + 5.];
                image.fill(bounds, [214, 214, 208, 255]);
                image.frame(bounds, [40, 40, 40, 255], 1.);
                for x in (26..256).step_by(54) {
                    image.oval(ellipse(f64::from(x), y, 2., 2.), [90, 90, 88, 255]);
                }
            }
            image
        }
        Rear::Mesh => mesh(rng, wear),
    };
    framed(image, rng, wear)
}

fn condenser_front(rng: &mut SeededRng, wear: f64, base: Rgb, ribs: bool) -> Canvas {
    let mut image = sheet_metal(rng, wear, base);
    // Geometry in dressing/aircon.rs uses these same opening proportions.
    let [cx, cy, rx, ry] = [0.37, 0.5, 0.275, 0.4].map(|value| value * SIDE);
    for (extra, color, width) in [
        (15., [255, 255, 255, 90], 1.),
        (12.5, [60, 60, 58, 100], 2.),
        (7., [255, 255, 255, 70], 1.),
        (4.5, [40, 40, 40, 160], 2.),
    ] {
        image.oval_with_outline(
            ellipse(cx, cy, rx + extra, ry + extra),
            None,
            Some(color),
            width,
        );
    }
    image.oval(ellipse(cx, cy, rx, ry), [16, 18, 20, 255]);
    let seam = cx + rx + 22.;
    image.line(&[[seam, 6.], [seam, SIDE - 6.]], [50, 50, 48, 140], 1.);
    image.line(
        &[[seam + 1., 6.], [seam + 1., SIDE - 6.]],
        [255, 255, 255, 100],
        1.,
    );
    if ribs {
        for x in (seam as u32 + 12..246).step_by(9) {
            let x = f64::from(x);
            image.line(&[[x, 70.], [x, SIDE - 24.]], [60, 60, 58, 100], 1.);
            image.line(
                &[[x + 1., 70.], [x + 1., SIDE - 24.]],
                [255, 255, 255, 100],
                1.,
            );
        }
    }
    image.rounded(
        [seam + 10., 16., SIDE - 14., 34.],
        3.,
        Some([92, 96, 102, 255]),
        None,
        1.,
    );
    image.fill([seam + 15., 22., SIDE - 34., 27.], [210, 212, 214, 255]);
    if wear < 0.6 {
        let (x, y) = (seam + 12., 40.);
        let bounds = [x, y, x + 34., y + 24.];
        image.fill(bounds, [236, 236, 228, 255]);
        image.frame(bounds, [90, 90, 86, 255], 1.);
        for (row, tone) in [
            [60, 140, 80],
            [150, 180, 70],
            [226, 196, 70],
            [214, 120, 60],
        ]
        .into_iter()
        .enumerate()
        {
            let t = row as f64 * 5.;
            image.fill(
                [x + 3., y + 3. + t, x + 12. + t, y + 6. + t],
                ink(tone, 255),
            );
        }
    }
    for x in [10., SIDE - 10.] {
        for y in [10., SIDE - 10.] {
            screw(&mut image, x, y, wear > 0.5);
        }
    }
    image
}

fn valve_side(rng: &mut SeededRng, wear: f64) -> Canvas {
    let mut image = sheet_metal(rng, wear, PAINT);
    for y in (22..86).step_by(9) {
        let y = f64::from(y);
        image.fill([34., y, SIDE - 34., y + 3.], [26, 28, 30, 235]);
        image.line(
            &[[34., y + 4.], [SIDE - 34., y + 4.]],
            [255, 255, 255, 130],
            1.,
        );
    }
    image.rounded(
        [40., 118., SIDE - 40., SIDE - 22.],
        10.,
        None,
        Some([50, 50, 48, 170]),
        2.,
    );
    image.rounded(
        [42., 120., SIDE - 40., SIDE - 22.],
        10.,
        None,
        Some([255, 255, 255, 80]),
        1.,
    );
    label(&mut image, 60., 134., 72., 44., rng);
    image.polygon(
        &[[150., 178.], [176., 178.], [163., 156.]],
        Some([226, 190, 60, 255]),
        Some([40, 40, 40, 255]),
    );
    image.fill([162., 163., 164., 172.], [40, 40, 40, 255]);
    for x in [96., 150.] {
        image.oval_with_outline(
            ellipse(x, SIDE - 47., 9., 9.),
            Some([150, 124, 80, 255]),
            Some([50, 50, 48, 255]),
            1.,
        );
        image.oval(ellipse(x, SIDE - 47., 4., 4.), [96, 78, 50, 255]);
    }
    image
}

fn fan(blades: u8, dusty: bool) -> Canvas {
    let mut image = Canvas::new(2, [44, 46, 48]);
    let center = SIDE * 0.5;
    let circle = |r| ellipse(center, center, r, r);
    for (radius, tone) in [(124., 70), (100., 92), (70., 78)] {
        image.oval(circle(radius), [tone, tone + 1, tone + 2, 255]);
    }
    let blade = if dusty {
        [236, 240, 242]
    } else {
        [160, 164, 170]
    };
    let point =
        |angle: f64, reach: f64| [center + angle.cos() * reach, center + angle.sin() * reach];
    for index in 0..blades {
        let angle = f64::from(index) * TAU / f64::from(blades) + 0.3;
        let sweep = TAU / f64::from(blades) * 0.72;
        let mut points = vec![point(angle, 22.)];
        for step in 0..=12 {
            let t = f64::from(step);
            points.push(point(
                angle + sweep * t / 12.,
                112. - 10. * (t - 7.).abs() / 7.,
            ));
        }
        points.push(point(angle + sweep * 0.8, 22.));
        image.polygon(&points, Some(ink(blade, 255)), None);
        image.line(&points[1..8], ink(tinted(blade, 1.3), 255), 3.);
        image.line(&points[..2], ink(tinted(blade, 0.6), 255), 2.);
    }
    image.oval_with_outline(
        circle(30.),
        Some(ink(tinted(blade, 0.8), 255)),
        Some([60, 60, 62, 255]),
        1.,
    );
    image.oval(circle(10.), [100, 100, 104, 255]);
    image.blur(1.2);
    image
}

fn plywood(rng: &mut SeededRng) -> Canvas {
    let mut image = Canvas::new(2, [188, 160, 118]);
    image.shade(&smooth(PANEL, [8; 2], 104, 140, rng));
    image.shade(&grain(PANEL, 4., rng));
    for _ in 0..26 {
        let y = rng.float(0., SIDE);
        image.line(
            &[[0., y], [SIDE, y + rng.float(-5., 5.)]],
            [110, 84, 52, 60],
            1.,
        );
    }
    weathered(image, rng, 0.45, false)
}

fn duct(rng: &mut SeededRng) -> Canvas {
    let mut image = sheet_metal(rng, 0.25, [240, 240, 236]);
    for y in [0., 64., 128., 192.] {
        image.line(&[[0., y], [SIDE, y]], [60, 60, 58, 150], 1.);
        image.line(&[[0., y + 1.], [SIDE, y + 1.]], [255, 255, 255, 140], 1.);
    }
    image.line(
        &[[SIDE * 0.5, 0.], [SIDE * 0.5, SIDE]],
        [60, 60, 58, 50],
        1.,
    );
    image
}

// Cell order is shared with the compiler's HVAC geometry atlas coordinates.
pub(super) fn tile(index: usize, rng: &mut SeededRng) -> Canvas {
    match index {
        0..=2 => side_panel(rng, [0.1, 0.45, 0.85][index]),
        3..=4 => top_panel(rng, [0.2, 0.9][index - 3]),
        5..=6 => fins(rng, [0.2, 0.8][index - 5]),
        7 => mesh(rng, 0.4),
        8 => condenser_front(rng, 0.1, PAINT, false),
        9 => condenser_front(rng, 0.35, CREAM, true),
        10 => condenser_front(rng, 0.8, CREAM, false),
        11 => condenser_front(rng, 0.3, GRAY, true),
        12..=13 => valve_side(rng, [0.15, 0.7][index - 12]),
        14 => fan(3, false),
        15 => fan(5, true),
        16..=23 => {
            let offset = index - 16;
            let kind = [Rear::Louvers, Rear::Grid, Rear::Coil, Rear::Mesh][offset % 4];
            let wear = [0.15, 0.2, 0.3, 0.25, 0.8, 0.75, 0.9, 0.85][offset];
            rear(kind, rng, wear)
        }
        24 => plywood(rng),
        25 => duct(rng),
        _ => unreachable!("HVAC atlas index"),
    }
}
