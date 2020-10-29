/// Linearly interpolate `value` between `points`.
///
/// `points` must not be empty and contain unique x values sorted in ascending order.
/// If `value` is smaller than the first point or larger than the last it is clamped.
pub fn interpolate(value: f64, points: &[(f64, f64)]) -> f64 {
    assert!(!points.is_empty());
    assert!(points.windows(2).all(|window| window[0].0 < window[1].0));

    if value < points[0].0 {
        points[0].1
    } else if let Some(window) = points
        .windows(2)
        .find(|window| value >= window[0].0 && value < window[1].0)
    {
        // https://en.wikipedia.org/wiki/Linear_interpolation#Linear_interpolation_between_two_known_points
        let (x, x0, y0, x1, y1) = (value, window[0].0, window[0].1, window[1].0, window[1].1);
        y0 + (x - x0) * ((y1 - y0) / (x1 - x0))
    } else {
        points.last().unwrap().1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_approx_eq::assert_approx_eq;

    #[test]
    fn interpolate_() {
        let points = &[(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)];
        assert_approx_eq!(interpolate(0.5, points), 1.0);
        assert_approx_eq!(interpolate(1.0, points), 1.0);
        assert_approx_eq!(interpolate(1.5, points), 1.5);
        assert_approx_eq!(interpolate(2.0, points), 2.0);
        assert_approx_eq!(interpolate(2.5, points), 2.5);
        assert_approx_eq!(interpolate(3.0, points), 3.0);
        assert_approx_eq!(interpolate(3.5, points), 3.0);
    }

    #[test]
    fn interpolate_works_with_single_point() {
        let points = &[(1.0, 1.0)];
        assert_approx_eq!(interpolate(0.5, points), 1.0);
        assert_approx_eq!(interpolate(1.0, points), 1.0);
        assert_approx_eq!(interpolate(1.5, points), 1.0);
    }

    #[test]
    fn interpolate_works_with_varying_x_and_y_deltas() {
        let points = &[(0.0, 1.0), (1.0, 0.0), (3.0, 1.0)];
        assert_approx_eq!(interpolate(0.0, points), 1.0);
        assert_approx_eq!(interpolate(0.4, points), 0.6);
        assert_approx_eq!(interpolate(0.5, points), 0.5);
        assert_approx_eq!(interpolate(0.6, points), 0.4);
        assert_approx_eq!(interpolate(1.0, points), 0.0);
        assert_approx_eq!(interpolate(1.5, points), 0.25);
        assert_approx_eq!(interpolate(2.0, points), 0.5);
        assert_approx_eq!(interpolate(2.5, points), 0.75);
        assert_approx_eq!(interpolate(3.0, points), 1.0);
    }
}
