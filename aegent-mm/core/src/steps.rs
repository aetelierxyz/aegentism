use rust_decimal::Decimal;

pub fn divides(step: Decimal, value: Decimal) -> bool {
    (value % step).is_zero()
}

pub fn floor_quotient(numerator: Decimal, denominator: Decimal) -> Decimal {
    let remainder = numerator % denominator;
    let quotient = (numerator - remainder) / denominator;
    if remainder < Decimal::ZERO {
        quotient - Decimal::ONE
    } else {
        quotient
    }
}

pub fn ceil_quotient(numerator: Decimal, denominator: Decimal) -> Decimal {
    let remainder = numerator % denominator;
    let quotient = (numerator - remainder) / denominator;
    if remainder > Decimal::ZERO {
        quotient + Decimal::ONE
    } else {
        quotient
    }
}

pub fn triangular(count: u32) -> Decimal {
    let count = u64::from(count);
    Decimal::from(count * count.saturating_sub(1) / 2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn a_step_divides_its_multiples_only() {
        assert!(divides(dec!(0.01), dec!(56)));
        assert!(divides(dec!(0.01), dec!(56.70)));
        assert!(divides(dec!(0.05), dec!(1.15)));
        assert!(!divides(dec!(0.01), dec!(56.705)));
        assert!(!divides(dec!(0.05), dec!(1.12)));
    }

    #[test]
    fn floor_quotient_rounds_toward_negative_infinity() {
        assert_eq!(floor_quotient(dec!(190.17), dec!(0.022)), dec!(8644));
        assert_eq!(floor_quotient(dec!(7), dec!(2)), dec!(3));
        assert_eq!(floor_quotient(dec!(-7), dec!(2)), dec!(-4));
        assert_eq!(floor_quotient(dec!(8), dec!(2)), dec!(4));
        assert_eq!(floor_quotient(dec!(-8), dec!(2)), dec!(-4));
        assert_eq!(floor_quotient(dec!(0), dec!(2)), dec!(0));
    }

    #[test]
    fn ceil_quotient_rounds_toward_positive_infinity() {
        assert_eq!(ceil_quotient(dec!(117.83), dec!(0.022)), dec!(5356));
        assert_eq!(ceil_quotient(dec!(7), dec!(2)), dec!(4));
        assert_eq!(ceil_quotient(dec!(-7), dec!(2)), dec!(-3));
        assert_eq!(ceil_quotient(dec!(8), dec!(2)), dec!(4));
        assert_eq!(ceil_quotient(dec!(-8), dec!(2)), dec!(-4));
        assert_eq!(ceil_quotient(dec!(0), dec!(2)), dec!(0));
    }

    #[test]
    fn triangular_counts_pairs() {
        assert_eq!(triangular(0), dec!(0));
        assert_eq!(triangular(1), dec!(0));
        assert_eq!(triangular(2), dec!(1));
        assert_eq!(triangular(18), dec!(153));
        assert_eq!(triangular(20), dec!(190));
    }
}
