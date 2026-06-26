pub fn segment(used_percent: Option<f64>, pending: bool, show: bool) -> String {
    // If show is false, return empty string
    if !show {
        return String::new();
    }

    // Build the head: "AH" with optional percent
    let mut result = String::from("AH");

    // Add used_percent if Some and finite
    if let Some(percent) = used_percent {
        if percent.is_finite() {
            let rounded = percent.round() as i32;
            result.push(' ');
            result.push_str(&format!("{}%", rounded));
        }
    }

    // Add pending indicator if needed
    if pending {
        result.push_str(" · ⏳1");
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_with_percent_no_pending() {
        assert_eq!(segment(Some(42.0), false, true), "AH 42%");
    }

    #[test]
    fn example_with_percent_and_pending() {
        assert_eq!(segment(Some(42.0), true, true), "AH 42% · ⏳1");
    }

    #[test]
    fn example_no_percent_no_pending() {
        assert_eq!(segment(None, false, true), "AH");
    }

    #[test]
    fn example_no_percent_with_pending() {
        assert_eq!(segment(None, true, true), "AH · ⏳1");
    }

    #[test]
    fn example_show_false() {
        assert_eq!(segment(Some(42.0), false, false), "");
    }

    #[test]
    fn rounding_down() {
        assert_eq!(segment(Some(41.4), false, true), "AH 41%");
    }

    #[test]
    fn rounding_up() {
        assert_eq!(segment(Some(41.6), false, true), "AH 42%");
    }

    #[test]
    fn nan_treated_as_no_percent() {
        assert_eq!(segment(Some(f64::NAN), false, true), "AH");
    }

    #[test]
    fn nan_with_pending() {
        assert_eq!(segment(Some(f64::NAN), true, true), "AH · ⏳1");
    }

    #[test]
    fn infinity_treated_as_no_percent() {
        assert_eq!(segment(Some(f64::INFINITY), false, true), "AH");
    }

    #[test]
    fn neg_infinity_treated_as_no_percent() {
        assert_eq!(segment(Some(f64::NEG_INFINITY), false, true), "AH");
    }
}
