use super::{
    Rng,
    builder::{Builder, Room},
};

pub(super) fn build(index: usize, b: &mut Builder, rng: &mut Rng) -> (Room, Room) {
    let wall = b.wall;
    match index {
        0 => {
            let (w, gap, run) = (
                rng.int(9, 12) as f64,
                rng.int(5, 7) as f64,
                rng.int(30, 38) as f64,
            );
            let total = 3. * w + 2. * gap;
            let start = b.room("muster", 0., 0., total, 10., 0., wall);
            let end = b.room("terminus", 0., 10. + run, total, 10., 0., wall);
            let mut lanes = vec![];
            for i in 0..3 {
                let x = i as f64 * (w + gap);
                let z = if i == 1 { 1.2 } else { 0. };
                let kind = if i == 1 { "ramp" } else { "floor" };
                b.surface(
                    &format!("lane{i}.entry"),
                    vec![[x, 10., 0.], [x + w, 10., 0.], [x + w, 18., z], [x, 18., z]],
                    kind,
                    wall,
                );
                lanes.push(b.room(format!("lane{i}"), x, 18., w, run - 16., z, wall));
                b.surface(
                    &format!("lane{i}.exit"),
                    vec![
                        [x, run + 2., z],
                        [x + w, run + 2., z],
                        [x + w, run + 10., 0.],
                        [x, run + 10., 0.],
                    ],
                    kind,
                    wall,
                );
            }
            b.join("cross.west", &lanes[0], &lanes[1], 4.);
            b.join("cross.east", &lanes[1], &lanes[2], 4.);
            (start, end)
        }
        1 => {
            let w = 12.;
            let pitch = w + rng.int(12, 18) as f64;
            let rooms = [(0., 0.), (pitch, 0.), (pitch, pitch), (0., pitch)]
                .into_iter()
                .enumerate()
                .map(|(i, (x, y))| b.room(format!("court{i}"), x, y, w, w, 0., wall))
                .collect::<Vec<_>>();
            for i in 0..4 {
                b.join(&format!("arc{i}"), &rooms[i], &rooms[(i + 1) % 4], 5.);
            }
            (rooms[0].clone(), rooms[2].clone())
        }
        2 => {
            let w = rng.int(12, 16) as f64;
            let pitch = w + 16.;
            let h = 4.4;
            let low = (0..2)
                .map(|i| {
                    b.room(
                        format!("lower{i}"),
                        i as f64 * pitch,
                        0.,
                        w,
                        12.,
                        0.,
                        h - 0.22,
                    )
                })
                .collect::<Vec<_>>();
            let high = (0..2)
                .map(|i| b.room(format!("upper{i}"), i as f64 * pitch, 0., w, 12., h, wall))
                .collect::<Vec<_>>();
            b.join("lower.hall", &low[0], &low[1], 5.);
            b.join("upper.hall", &high[0], &high[1], 5.);
            for i in 0..2 {
                b.switchback(&format!("stairs{i}"), &low[i], &high[i]);
            }
            (low[0].clone(), high[1].clone())
        }
        3 => {
            let w = 12.;
            let pitch = w + rng.int(8, 12) as f64;
            let mut rooms = vec![];
            for y in 0..3 {
                let mut row = vec![];
                for x in 0..3 {
                    row.push(b.room(
                        format!("block{x}.{y}"),
                        x as f64 * pitch,
                        y as f64 * pitch,
                        w,
                        w,
                        if x == 1 && y == 1 { 1.2 } else { 0. },
                        wall,
                    ));
                }
                rooms.push(row);
            }
            for y in 0..3 {
                for x in 0..3 {
                    if x < 2 {
                        b.join(&format!("east{x}.{y}"), &rooms[y][x], &rooms[y][x + 1], 5.);
                    }
                    if y < 2 {
                        b.join(&format!("north{x}.{y}"), &rooms[y][x], &rooms[y + 1][x], 5.);
                    }
                }
            }
            (rooms[0][0].clone(), rooms[1][1].clone())
        }
        4 => {
            let gap = rng.int(22, 28) as f64;
            let x = 12. + gap;
            let h = 4.;
            let west = b.room("west.bank", 0., 0., 12., 36., h, 1.15);
            let east = b.room("east.bank", x, 0., 12., 36., h, 1.15);
            for (i, y) in [6., 28.].into_iter().enumerate() {
                b.surface(
                    &format!("span{i}"),
                    vec![
                        [12., y - 2., h],
                        [x, y - 2., h],
                        [x, y + 2., h],
                        [12., y + 2., h],
                    ],
                    "floor",
                    1.15,
                );
            }
            b.room("gorge", 12., 0., gap, 60., 0., 1.15);
            let low_w = b.room("west.landing", 2., 52., 8., 8., 0., 1.15);
            let low_e = b.room("east.landing", x + 2., 52., 8., 8., 0., 1.15);
            b.join("west.descent", &west, &low_w, 4.);
            b.join("east.descent", &east, &low_e, 4.);
            b.surface(
                "west.low",
                vec![
                    [10., 54., 0.],
                    [12., 54., 0.],
                    [12., 58., 0.],
                    [10., 58., 0.],
                ],
                "floor",
                1.15,
            );
            b.surface(
                "east.low",
                vec![
                    [x, 54., 0.],
                    [x + 2., 54., 0.],
                    [x + 2., 58., 0.],
                    [x, 58., 0.],
                ],
                "floor",
                1.15,
            );
            (west, east)
        }
        5 => {
            let cols = rng.int(3, 4);
            let pitch = rng.int(20, 24) as f64;
            let mut rooms = vec![];
            for y in 0..2 {
                let mut row = vec![];
                for x in 0..cols {
                    let z = 6. + if rng.int(0, 1) == 0 { 0. } else { 0.8 };
                    row.push(b.room(
                        format!("island{x}.{y}"),
                        x as f64 * pitch,
                        y as f64 * pitch,
                        12.,
                        12.,
                        z,
                        1.15,
                    ));
                }
                rooms.push(row);
            }
            for (y, row) in rooms.iter().enumerate() {
                for x in 0..cols - 1 {
                    b.join(&format!("walk{x}.{y}"), &row[x], &row[x + 1], 4.);
                }
            }
            b.join("west.walk", &rooms[0][0], &rooms[1][0], 4.);
            b.join("east.walk", &rooms[0][cols - 1], &rooms[1][cols - 1], 4.);
            (rooms[0][0].clone(), rooms[1][cols - 1].clone())
        }
        _ => {
            let w = rng.int(38, 44) as f64;
            let cx = w / 2.;
            let south = b.room("south.deck", 0., 0., w, 10., 2., 1.15);
            let north = b.room("north.deck", 0., 26., w, 10., 2., 1.15);
            let west = b.room("west.deck", 0., 10., cx - 8., 16., 2., 1.15);
            let east = b.room("east.deck", cx + 8., 10., cx - 8., 16., 2., 1.15);
            let pit = b.room("pit", cx - 4., 10., 8., 16., 0., 1.15);
            b.join("pit.west", &west, &pit, 6.);
            b.join("pit.east", &pit, &east, 6.);
            (south, north)
        }
    }
}
