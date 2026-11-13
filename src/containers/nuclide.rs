use mendeleev::Element;
use nucleide_nuclei::NuclideId;
use physure_core::quantity::Quantity;
use serde::{Deserialize, Serialize};

use super::physure_helper::{bq_per_g, kev, opt_quantity_serde, percent, quantity_serde, seconds};

// ---------------------------------------------------------------------------
// NuclideId serde bridges
// ---------------------------------------------------------------------------

mod nuclide_id_string {
    use nucleide_nuclei::NuclideId;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(id: &NuclideId, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(id.to_name().as_str())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<NuclideId, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        NuclideId::from_name(&s).map_err(serde::de::Error::custom)
    }
}

mod opt_nuclide_id_string {
    use nucleide_nuclei::NuclideId;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(id: &Option<NuclideId>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match id {
            Some(id) => serializer.serialize_some(id.to_name().as_str()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<NuclideId>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt = Option::<String>::deserialize(deserializer)?;
        match opt {
            Some(s) => NuclideId::from_name(&s)
                .map(Some)
                .map_err(serde::de::Error::custom),
            None => Ok(None),
        }
    }
}

mod vec_nuclide_id_string_string_f64 {
    use nucleide_nuclei::NuclideId;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(v: &Vec<(NuclideId, String, f64)>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let converted: Vec<(String, String, f64)> = v
            .iter()
            .map(|(id, daughter_type, e)| (id.to_name(), daughter_type.clone(), *e))
            .collect();
        converted.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<(NuclideId, String, f64)>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw: Vec<(String, String, f64)> = Vec::deserialize(deserializer)?;
        raw.into_iter()
            .map(|(s, daughter_type, e)| {
                NuclideId::from_name(&s)
                    .map(|id| (id, daughter_type, e))
                    .map_err(serde::de::Error::custom)
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Controlled vocabularies
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EmissionType {
    Gamma,
    Annihilation,
    BetaMinus,
    BetaPlus,
    Alpha,
    XRay,
    ConversionElectron,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EmissionOrigin {
    Annihilation,
    NuclearDecay,
}

// ---------------------------------------------------------------------------
// Emission
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Emission {
    /// `None` for placeholder / annihilation emissions, where there is no
    /// single parent nuclide.
    #[serde(with = "opt_nuclide_id_string")]
    pub parent_nuclide: Option<NuclideId>,

    /// Energy in keV, with uncertainty folded in.
    #[serde(with = "opt_quantity_serde::kev")]
    pub energy_kev: Option<Quantity>,

    /// Intensity as percent, with uncertainty folded in.
    #[serde(with = "opt_quantity_serde::percent")]
    pub intensity_percent: Option<Quantity>,

    pub r#type: Option<EmissionType>,
    pub origin: Option<EmissionOrigin>,
}

impl Emission {
    /// Placeholder equivalent to Python's `EmptyEmission`.
    pub fn empty() -> Self {
        Self {
            parent_nuclide: None,
            energy_kev: None,
            intensity_percent: None,
            r#type: None,
            origin: None,
        }
    }

    /// Equivalent to Python's `AnnihilationEmission`.
    pub fn annihilation() -> Self {
        Self {
            parent_nuclide: None,
            energy_kev: Some(Quantity::new_scalar(511.0, 1.0, kev(), None, None)),
            intensity_percent: Some(Quantity::new_scalar(100.0, 1.0, percent(), None, None)),
            r#type: Some(EmissionType::Annihilation),
            origin: Some(EmissionOrigin::Annihilation),
        }
    }
}

// ---------------------------------------------------------------------------
// Nuclide
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Nuclide {
    /// Canonical nuclide identifier from `nucleide-nuclei`.
    #[serde(with = "nuclide_id_string")]
    pub nuclide: NuclideId,

    /// Chemical element from `mendeleev::Element`.
    pub element: Element,

    #[serde(with = "vec_nuclide_id_string_string_f64")]
    pub daughters: Vec<(NuclideId, String, f64)>,

    /// Half-life with uncertainty (seconds).
    #[serde(with = "quantity_serde::seconds")]
    pub half_life_s: Quantity,

    /// Specific activity with uncertainty (Bq/g).
    #[serde(with = "quantity_serde::bq_per_g")]
    pub specific_activity_bq_per_g: Quantity,

    pub citation_ref: String,

    #[serde(default)]
    pub emissions: Vec<Emission>,
}
