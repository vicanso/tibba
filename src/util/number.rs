pub fn float_to_fixed(value: f64, precision: usize) -> String {
    match precision {
        0 => format!("{:.0}", value),
        1 => format!("{:.1}", value),
        2 => format!("{:.2}", value),
        3 => format!("{:.3}", value),
        _ => format!("{:.4}", value),
    }
}

#[cfg(test)]
mod tests {
    use super::float_to_fixed;
    use pretty_assertions::assert_eq;
    #[test]
    fn to_fixed() {
        assert_eq!("1", float_to_fixed(1.123412, 0));
        assert_eq!("1.1", float_to_fixed(1.123412, 1));
        assert_eq!("1.12", float_to_fixed(1.123412, 2));
        assert_eq!("1.123", float_to_fixed(1.123412, 3));
        assert_eq!("1.1234", float_to_fixed(1.123412, 4));
    }
}
