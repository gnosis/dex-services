use anyhow::{anyhow, Error};
use std::convert::TryFrom;

/// Not empty and contains unique x values sorted in ascending order.
#[derive(Copy, Clone, Debug)]
pub struct Points<'a>(&'a [(f64, f64)]);

impl<'a> TryFrom<&'a [(f64, f64)]> for Points<'a> {
    type Error = Error;

    fn try_from(points: &'a [(f64, f64)]) -> Result<Self, Self::Error> {
        let is_finite = points
            .iter()
            .all(|point| point.0.is_finite() && point.1.is_finite());
        let is_sorted_and_unique = points.windows(2).all(|window| window[0].0 < window[1].0);
        if points.is_empty() {
            Err(anyhow!("points is empty"))
        } else if !is_finite {
            Err(anyhow!("points contains non finite value"))
        } else if !is_sorted_and_unique {
            Err(anyhow!("points is not sorted an unique"))
        } else {
            Ok(Self(points))
        }
    }
}

/// Linearly interpolate `value` between `points`.
///
/// If `value` is smaller than the first point or larger than the last it is clamped.
pub fn interpolate(value: f64, points: Points) -> f64 {
    let points = points.0;
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
        let points = Points::try_from([(1.0, 1.0), (2.0, 2.0), (3.0, 3.0)].as_ref()).unwrap();
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
        let points = Points::try_from([(1.0, 1.0)].as_ref()).unwrap();
        assert_approx_eq!(interpolate(0.5, points), 1.0);
        assert_approx_eq!(interpolate(1.0, points), 1.0);
        assert_approx_eq!(interpolate(1.5, points), 1.0);
    }

    #[test]
    fn interpolate_works_with_varying_x_and_y_deltas() {
        let points = Points::try_from([(0.0, 1.0), (1.0, 0.0), (3.0, 1.0)].as_ref()).unwrap();
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

    #[test]
    fn points_must_not_be_empty() {
        assert!(Points::try_from([].as_ref()).is_err());
    }

    #[test]
    fn points_must_not_be_nan() {
        assert!(Points::try_from([(f64::NAN, 0.0)].as_ref()).is_err());
        assert!(Points::try_from([(0.0, f64::NAN)].as_ref()).is_err());
    }

    #[test]
    fn points_must_be_sorted() {
        assert!(Points::try_from([(1.0, 0.0), (0.0, 0.0)].as_ref()).is_err());
    }

    #[test]
    fn points_must_be_unique() {
        assert!(Points::try_from([(0.0, 0.0), (0.0, 1.0)].as_ref()).is_err());
    }
}
