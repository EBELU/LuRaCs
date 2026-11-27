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

#[cfg(test)]
mod tests {
    use super::*;
    use rsplot::egui::Color32;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Palette {
        #[serde(with = "c")]
        primary: Color32,
        #[serde(with = "vec")]
        list: Vec<Color32>,
        #[serde(with = "opt")]
        accent: Option<Color32>,
    }

    #[test]
    fn test_serde_color_roundtrip_and_formats() {
        let p = Palette {
            primary: Color32::from_rgb(0x12, 0x34, 0x56),
            list: vec![
                Color32::from_rgb(0xaa, 0xbb, 0xcc),
                Color32::from_rgba_unmultiplied(0x10, 0x20, 0x30, 0x80),
            ],
            accent: Some(Color32::from_rgba_unmultiplied(0xff, 0x00, 0x00, 0x40)),
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("#123456"));
        assert!(json.contains("#aabbcc"));
        assert!(json.contains("#ff000040"));

        // `vec::deserialize` uses `Color32`'s `[u8; 4]` deserializer.
        let vec_json = r##"{"primary":"#123456","list":[[10,20,30,255]],"accent":null}"##;
        let decoded: Palette = serde_json::from_str(vec_json).unwrap();
        assert_eq!(decoded.primary, p.primary);
        assert_eq!(decoded.list, vec![Color32::from_rgba_unmultiplied(10, 20, 30, 255)]);

        // RGB / RGBA array forms & None
        let raw_json = r##"{"primary":[10,20,30],"list":[],"accent":[1,2,3,4]}"##;
        let p2: Palette = serde_json::from_str(raw_json).unwrap();
        assert_eq!(p2.primary, Color32::from_rgb(10, 20, 30));
        assert_eq!(p2.accent, Some(Color32::from_rgba_unmultiplied(1, 2, 3, 4)));

        let raw_none = r##"{"primary":"#11223344","list":[],"accent":null}"##;
        let p3: Palette = serde_json::from_str(raw_none).unwrap();
        assert_eq!(p3.accent, None);
        assert!(serde_json::to_string(&p3).unwrap().contains("null"));

        // Invalid hex
        let bad = r##"{"primary":"zzzzzz","list":[],"accent":null}"##;
        assert!(serde_json::from_str::<Palette>(bad).is_err());
        let bad_opt = r##"{"primary":"#112233","list":[],"accent":"bad"}"##;
        assert!(serde_json::from_str::<Palette>(bad_opt).is_err());
    }
}
