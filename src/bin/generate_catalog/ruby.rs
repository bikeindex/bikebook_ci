//! Ruby's semantics where the published catalog depends on them

use serde_json::Value;
use std::cmp::Ordering;

/// `blank?`
pub fn blank(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(false)) => true,
        Some(Value::String(string)) => string.trim().is_empty(),
        Some(Value::Array(items)) => items.is_empty(),
        Some(Value::Object(map)) => map.is_empty(),
        _ => false,
    }
}

/// `Array(value)`, but for a hash, which no field this reads is
pub fn array(value: Option<&Value>) -> Vec<&Value> {
    match value {
        None | Some(Value::Null) => vec![],
        Some(Value::Array(items)) => items.iter().collect(),
        Some(other) => vec![other],
    }
}

/// Truthy, as `filter_map` keeps it
pub fn truthy(value: Option<&Value>) -> Option<&Value> {
    value.filter(|value| !matches!(value, Value::Null | Value::Bool(false)))
}

pub fn float(value: &Value) -> f64 {
    value.as_f64().unwrap_or_default()
}

fn compare(left: &Value, right: &Value) -> Ordering {
    float(left).total_cmp(&float(right))
}

/// `min`, which keeps the first of equals
pub fn min<'a>(values: impl IntoIterator<Item = &'a Value>) -> Option<&'a Value> {
    values.into_iter().fold(None, |least, value| match least {
        Some(least) if compare(least, value) != Ordering::Greater => Some(least),
        _ => Some(value),
    })
}

/// `max`, which keeps the first of equals
pub fn max<'a>(values: impl IntoIterator<Item = &'a Value>) -> Option<&'a Value> {
    values.into_iter().fold(None, |most, value| match most {
        Some(most) if compare(most, value) != Ordering::Less => Some(most),
        _ => Some(value),
    })
}

/// `Array#sum`: integers exactly, then Kahan-Babuska summation from the first float on
pub fn sum<'a>(values: impl IntoIterator<Item = &'a Value>) -> Value {
    let mut values = values.into_iter().peekable();
    let mut integer: i128 = 0;
    while let Some(next) = values.peek().and_then(|value| value.as_i64()) {
        integer += i128::from(next);
        values.next();
    }
    if values.peek().is_none() {
        return Value::from(integer as i64);
    }
    Value::from(float_sum(integer as f64, values.map(float)))
}

/// The float half of `Array#sum`
pub fn float_sum(start: f64, values: impl IntoIterator<Item = f64>) -> f64 {
    let (total, compensation) =
        values
            .into_iter()
            .fold((start, 0.0), |(total, compensation), value| {
                let next = total + value;
                let lost = if total.abs() >= value.abs() {
                    (total - next) + value
                } else {
                    (value - next) + total
                };
                (next, compensation + lost)
            });
    total + compensation
}

/// `Float#round(digits)` for digits above 0, rounding half away from zero
pub fn round(number: f64, digits: i32) -> f64 {
    if number == 0.0 || !number.is_finite() {
        return number;
    }
    let scale = 10f64.powi(digits);
    let scaled = (number * scale).round();
    let rounded = if number > 0.0 && (scaled + 0.5) / scale <= number {
        scaled + 1.0
    } else if number < 0.0 && (scaled - 0.5) / scale >= number {
        scaled - 1.0
    } else {
        scaled
    };
    rounded / scale
}

/// `Float#round(-1)` of a positive number: to its tens, from its integer part
pub fn round_to_tens(number: f64) -> i64 {
    (number.trunc() as i64 + 5).div_euclid(10) * 10
}

/// Catalog::Comparison's median: of the middle two, to three places
pub fn median<'a>(values: impl IntoIterator<Item = &'a Value>) -> Option<f64> {
    let mut sorted: Vec<&Value> = values.into_iter().collect();
    if sorted.is_empty() {
        return None;
    }
    sorted.sort_by(|left, right| compare(left, right));
    let (low, high) = (sorted[(sorted.len() - 1) / 2], sorted[sorted.len() / 2]);
    let middle = match (low.as_i64(), high.as_i64()) {
        (Some(low), Some(high)) => (low + high) as f64,
        _ => float(low) + float(high),
    };
    Some(round(middle / 2.0, 3))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sum_is_exact_over_integers_and_compensated_over_floats() {
        assert_eq!(sum(&[json!(1), json!(2)]), json!(3));
        let values: Vec<Value> = [0.1; 10].iter().map(|value| json!(value)).collect();
        assert_eq!(sum(&values), json!(1.0));
    }

    #[test]
    fn round_corrects_a_half_the_scaling_lost() {
        assert_eq!(round(1.0005, 3), 1.001);
        assert_eq!(round(2.675, 2), 2.68);
        assert_eq!(round(-2.5, 0), -3.0);
    }

    #[test]
    fn round_to_tens_rounds_half_up() {
        assert_eq!(round_to_tens(1634.9), 1630);
        assert_eq!(round_to_tens(1635.0), 1640);
    }

    #[test]
    fn median_averages_the_middle_two() {
        assert_eq!(
            median(&[json!(3), json!(1), json!(2), json!(10)]),
            Some(2.5)
        );
        assert_eq!(median(&[]), None);
    }

    #[test]
    fn min_and_max_keep_the_first_of_equals() {
        let values = [json!(2), json!(2.0)];
        assert_eq!(min(&values), Some(&json!(2)));
        assert_eq!(max(&values), Some(&json!(2)));
    }

    #[test]
    fn blank_counts_false_and_whitespace() {
        assert!(blank(Some(&json!(false))));
        assert!(blank(Some(&json!(" "))));
        assert!(!blank(Some(&json!(0))));
    }
}
