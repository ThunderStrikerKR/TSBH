//! Display-only monetary formatting; no business price calculation.
/// Display-only won formatting. No business price or rounding decisions live
/// in the UI layer; amounts are supplied by the Rust business engine.
pub fn display_won(amount: i64) -> String {
    format!("{}원",display_money_number(amount))
}

pub(crate) fn display_money_number(amount:i64)->String{
    let digits = amount.to_string();
    let (sign, positive) = if let Some(text) = digits.strip_prefix('-') {
        ("-", text)
    } else { ("", digits.as_str()) };
    let mut groups = String::new();
    for (index, character) in positive.chars().enumerate() {
        if index > 0 && (positive.len() - index) % 3 == 0 {
            groups.push(',');
        }
        groups.push(character);
    }
    format!("{sign}{groups}")
}

