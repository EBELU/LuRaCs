pub mod c {
    use rsplot::egui::Color32;
    use serde::{Deserialize, Deserializer, Serializer};

    use crate::utils::serde_color::helper;

    pub fn serialize<S: Serializer>(
        color: &Color32,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        let [r, g, b, a] = color.to_srgba_unmultiplied();
        if a == 255 {
            serializer.serialize_str(&format!("#{r:02x}{g:02x}{b:02x}"))
        } else {
            serializer.serialize_str(&format!("#{r:02x}{g:02x}{b:02x}{a:02x}"))
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Color32, D::Error> {
        match helper::Raw::deserialize(deserializer)? {
            helper::Raw::Hex(s) => helper::parse_hex(&s)
                .ok_or_else(|| serde::de::Error::custom(format!("invalid hex colour: {s:?}"))),
            helper::Raw::Rgb([r, g, b]) => Ok(Color32::from_rgb(r, g, b)),
            helper::Raw::Rgba([r, g, b, a]) => Ok(Color32::from_rgba_unmultiplied(r, g, b, a)),
        }
    }
}

pub mod vec {
    use rsplot::egui::Color32;
    use serde::{Deserialize, Deserializer, Serializer, ser::SerializeSeq};

    use super::c;

    pub fn serialize<S: Serializer>(
        colors: &Vec<Color32>,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(colors.len()))?;
        for color in colors {
            let [r, g, b, a] = color.to_srgba_unmultiplied();
            let value = if a == 255 {
                format!("#{r:02x}{g:02x}{b:02x}")
            } else {
                format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
            };
            seq.serialize_element(&value)?;
        }
        seq.end()
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Vec<Color32>, D::Error> {
        Vec::<Color32>::deserialize(deserializer)
    }
}

// the same for optional colors
pub mod opt {
    use crate::utils::serde_color::helper;

    use super::c;
    use rsplot::egui::Color32;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        color: &Option<Color32>,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        match color {
            Some(c) => c::serialize(c, serializer),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Option<Color32>, D::Error> {
        let opt = Option::<helper::Raw>::deserialize(deserializer)?;
        match opt {
            Some(helper::Raw::Hex(s)) => helper::parse_hex(&s)
                .ok_or_else(|| serde::de::Error::custom(format!("invalid hex colour: {s:?}")))
                .map(Some),
            Some(helper::Raw::Rgb([r, g, b])) => Ok(Some(Color32::from_rgb(r, g, b))),
            Some(helper::Raw::Rgba([r, g, b, a])) => {
                Ok(Some(Color32::from_rgba_unmultiplied(r, g, b, a)))
            }
            None => Ok(None),
        }
    }
}

mod helper {
    use rsplot::egui::Color32;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(untagged)]
    pub enum Raw {
        Hex(String),
        Rgb([u8; 3]),
        Rgba([u8; 4]),
    }

    pub fn parse_hex(s: &str) -> Option<Color32> {
        let hex = s.strip_prefix('#').unwrap_or(s);
        match hex.len() {
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                Some(Color32::from_rgb(r, g, b))
            }
            8 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
                Some(Color32::from_rgba_unmultiplied(r, g, b, a))
            }
            _ => None,
        }
    }
}
