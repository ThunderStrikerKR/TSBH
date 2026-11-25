//! Read-only product color/option presentation. Never use this to rewrite
//! catalog values, variant identifiers, order evidence, or exported data.
use serde_json::Value;

fn field<'a>(row: &'a Value, key: &str) -> &'a str {
    row.get(key).and_then(Value::as_str).unwrap_or("")
}

fn color_label(color: &str) -> String {
    match color.trim().to_lowercase().as_str() {
        "gold"=>"골드","rose gold"=>"로즈골드","cosmo black"=>"코스모블랙",
        "black"|"검정"|"블랙"=>"블랙","gray"|"grey"|"회색"|"그레이"=>"그레이",
        "brown"|"브라운"=>"브라운","white"|"흰색"|"화이트"=>"화이트",
        "clear"|"클리어"=>"클리어","frost"|"frosted"|"프로스트"=>"프로스트",
        "silver"=>"실버","blue"=>"블루","red"=>"레드","단일"|"색상 미표기"=>"",
        _=>color.trim()
    }.to_owned()
}

/// One selected sellable variant. An actual option remains visible even if
/// the color is absent or is explicitly not required.
pub(crate) fn variant_label(product: &Value, variant: &Value) -> String {
    let color = if field(product, "colorStatus") == "NOT_REQUIRED" {
        String::new()
    } else {
        color_label(field(variant, "color"))
    };
    let option = field(variant, "option").trim();
    match (color.is_empty(), option.is_empty()) {
        (true, true) => String::new(),
        (true, false) => option.to_owned(),
        (false, true) => color,
        (false, false) => format!("{color} / {option}"),
    }
}

/// One product row spanning all registered variants. Keep order and remove
/// duplicate visible labels, but never change the underlying variants.
pub(crate) fn product_label(product: &Value) -> String {
    let mut labels = Vec::new();
    if let Some(variants) = product["variants"].as_array() {
        for variant in variants {
            let label = variant_label(product, variant);
            if !label.is_empty() && !labels.contains(&label) {
                labels.push(label);
            }
        }
    }
    labels.join(" / ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn blank_placeholders_and_actual_options() {
        for color in ["단일", "색상 미표기", "", " 단일 "] {
            let product = json!({"variants":[{"color":color}]});
            assert_eq!(variant_label(&product, &product["variants"][0]), "");
            assert_eq!(product_label(&product), "");
        }
        let product = json!({"variants":[{"color":"단일","option":"35cm"},{"color":"Black","option":"50cm"},{"color":"Chrome"},{"color":"Black","option":"50cm"}]});
        assert_eq!(variant_label(&product, &product["variants"][0]), "35cm");
        assert_eq!(product_label(&product), "35cm / 블랙 / 50cm / Chrome");
        let no_color = json!({"colorStatus":"NOT_REQUIRED","variants":[{"color":"Red","option":"민판"}]});
        assert_eq!(product_label(&no_color), "민판");
        assert_eq!(product_label(&json!({"variants":[]})), "");
        assert_eq!(product["variants"][0]["color"], "단일");
    }
}
