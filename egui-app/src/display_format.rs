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


/// Freight label only; zero alone is never an exemption.
pub(crate) fn display_freight(amount:Option<i64>,exempt:bool,collect:bool)->String{
    if collect {"착불".to_owned()}
    else if exempt {"면제".to_owned()}
    else {amount.map(display_won).unwrap_or_else(||"—".to_owned())}
}
#[cfg(test)]
mod freight_tests{
    use super::*;
    #[test]
    fn freight_state_and_numeric_zero_are_distinct(){
        assert_eq!(display_freight(Some(0),true,false),"면제");
        assert_eq!(display_freight(Some(0),false,false),"0원");
        assert_eq!(display_freight(None,false,false),"—");
        assert_eq!(display_freight(Some(3500),false,false),"3,500원");
        assert_eq!(display_freight(Some(0),false,true),"착불");
    }
}
