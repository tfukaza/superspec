use crate::math::*;
pub fn polygon_area(points: &[V3]) -> f64 {
    if points.len() < 3 {
        return 0.;
    }
    (1..points.len() - 1)
        .map(|i| {
            length(cross(
                sub(points[i], points[0]),
                sub(points[i + 1], points[0]),
            )) / 2.
        })
        .sum()
}
pub fn half_plane(points: &[V3], distance: impl Fn(V3) -> f64) -> Vec<V3> {
    let mut result = vec![];
    for i in 0..points.len() {
        let p = points[i];
        let q = points[(i + 1) % points.len()];
        let a = distance(p);
        let b = distance(q);
        if a >= 0. {
            result.push(p);
        }
        if (a >= 0.) != (b >= 0.) {
            result.push(lerp(p, q, a / (a - b)));
        }
    }
    result
}
pub fn partition(
    points: &[V3],
    triangle: [V3; 3],
    project: impl Fn(V3) -> V3,
) -> (Vec<V3>, Vec<Vec<V3>>) {
    let mut projected = triangle.map(&project);
    if orient(projected[0], projected[1], projected[2]) < 0. {
        projected.reverse();
    }
    let mut inside = points.to_vec();
    let mut outside = vec![];
    for i in 0..3 {
        let (a, b) = (projected[i], projected[(i + 1) % 3]);
        let removed = half_plane(&inside, |p| -orient(a, b, project(p)));
        if polygon_area(&removed) > 1e-10 {
            outside.push(removed);
        }
        inside = half_plane(&inside, |p| orient(a, b, project(p)));
    }
    if polygon_area(&inside) <= 1e-10 {
        inside.clear();
    }
    (inside, outside)
}
pub fn dominant_axis(normal: V3) -> usize {
    (0..3)
        .max_by(|a, b| normal[*a].abs().total_cmp(&normal[*b].abs()))
        .unwrap()
}
pub fn project(p: V3, axis: usize) -> V3 {
    match axis {
        0 => [p[1], p[2], 0.],
        1 => [p[0], p[2], 0.],
        _ => [p[0], p[1], 0.],
    }
}
