//! Load `Nuclide` records from ENSDF-style JSON files.
//!
//! Matches the Python loader in behaviour except where the Python code was
//! inconsistent (see `Daughters` handling below).
//!
//! Behaviour preserved from Python:
//! - Emissions whose energy is within `np.isclose` tolerance of 511 keV are
//!   re-typed as `EmissionType::Annihilation`, regardless of the `"Type"`
//!   field, but their `"Origin"` field is still read from JSON.
//! - `parent_nuclide` is set to the parent for every emission (including
//!   the 511-keV ones) — matching the Python loader, not `Emission::annihilation()`.
//! - Missing `"Emissions"` is treated as empty.
//!
// Deviation from Python:
// - `Daughters` entries are `[decay_mode, daughter_name, branching]`.
//   The Python loader built `(decay_mode, daughter_name, branching)` triples
//   despite its `list[tuple[str, float]]` annotation, so the annotation was
//   never accurate. We drop `decay_mode` here (unused by anything downstream)
//   and store `(daughter, branching)` matching the annotation's intent.
//   If decay mode ever matters, widen `Nuclide.daughters` to
//   `Vec<(NuclideId, String, f64)>`.

use std::path::{Path, PathBuf};

use mendeleev::Element;
use nucleide_nuclei::NuclideId;
use physure_core::quantity::Quantity;
use serde_json::Value;
use thiserror::Error;

use crate::containers::{Emission, EmissionOrigin, EmissionType, Nuclide};
use crate::containers::{bq_per_g, kev, percent, seconds};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum NuclideLoadError {
    #[error("invalid glob pattern: {0}")]
    Glob(#[from] glob::PatternError),

    #[error("glob iteration error: {0}")]
    GlobIter(#[from] glob::GlobError),

    #[error("I/O error reading {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("JSON parse error in {path}: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },

    #[error("missing field {field:?} in {path}")]
    MissingField { path: PathBuf, field: &'static str },

    #[error("field {field:?} has unexpected JSON type {actual} in {path}")]
    WrongType {
        path: PathBuf,
        field: &'static str,
        actual: &'static str,
    },

    #[error("invalid nuclide identifier {id:?} in {path}")]
    InvalidNuclide { path: PathBuf, id: String },

    #[error("invalid element symbol {symbol:?} in {path}")]
    InvalidElement { path: PathBuf, symbol: String },

    #[error("invalid value for {field:?} in {path}: {reason}")]
    InvalidValue {
        path: PathBuf,
        field: &'static str,
        reason: String,
    },
}

pub type NuclideLoadResult<T> = std::result::Result<T, NuclideLoadError>;

/// Look up an element by symbol (`"Cs"`) or English name (`"Caesium"`),
/// case-insensitively. `mendeleev::Element` has no `FromStr` / `from_symbol`
/// in the 0.8.x line, so we scan `Element::iter()`.
///
/// Handles the parenthesised suffix some ENSDF entries carry — e.g.
/// `"Radon (Thoron)"` → `"Radon"`. The suffix names an isotope or an
/// alternative name; the element is what precedes it.
fn lookup_element(symbol_or_name: &str) -> Option<Element> {
    let name = symbol_or_name.trim();
    let base = name.split_once(" (").map(|(b, _)| b.trim()).unwrap_or(name);
    Element::iter()
        .find(|e| e.symbol().eq_ignore_ascii_case(base) || e.name().eq_ignore_ascii_case(base))
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

/// Load every JSON file matching `pattern` as a `Nuclide`. The pattern uses
/// standard glob syntax (`"data/*.json"`, `"data/*.json.gz"` if you add a
/// decompression step, etc.).
pub fn load_nuclide_data(pattern: &str) -> NuclideLoadResult<Vec<Nuclide>> {
    let mut out = Vec::new();
    for path in glob::glob(pattern)? {
        out.push(nuclide_from_file(&path?)?);
    }
    Ok(out)
}

pub fn nuclide_from_file(path: &Path) -> NuclideLoadResult<Nuclide> {
    let text = std::fs::read_to_string(path).map_err(|source| NuclideLoadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let value: Value = serde_json::from_str(&text).map_err(|source| NuclideLoadError::Json {
        path: path.to_path_buf(),
        source,
    })?;
    nuclide_from_json(&value, path)
}

pub fn nuclide_from_json(data: &Value, path: &Path) -> NuclideLoadResult<Nuclide> {
    let nuclide_name = get_str(data, "Nuclide", path)?;
    let nuclide_id =
        NuclideId::from_name(nuclide_name).map_err(|_| NuclideLoadError::InvalidNuclide {
            path: path.to_path_buf(),
            id: nuclide_name.to_string(),
        })?;

    let element_str = get_str(data, "Element", path)?;
    let element: Element =
        lookup_element(element_str).ok_or_else(|| NuclideLoadError::InvalidElement {
            path: path.to_path_buf(),
            symbol: element_str.to_string(),
        })?;

    let citation_ref = get_str(data, "citation_ref", path)?.to_string();

    let (hl_mean, hl_err) = get_pair(data, "Half-life (s)", path)?;
    let half_life_s = Quantity::new_scalar(hl_mean, hl_err, seconds(), None, None);

    let (sa_mean, sa_err) = get_pair(data, "Specific activity (Bq/g)", path)?;
    let specific_activity_bq_per_g = Quantity::new_scalar(sa_mean, sa_err, bq_per_g(), None, None);

    // Daughters: each entry is [decay_mode, daughter_name, branching].
    let daughters_arr = data
        .get("Daughters")
        .and_then(Value::as_array)
        .ok_or_else(|| missing_or_wrong(data, "Daughters", path))?;

    let mut daughters: Vec<(NuclideId, String, f64)> = Vec::with_capacity(daughters_arr.len());
    for entry in daughters_arr {
        let arr = entry
            .as_array()
            .ok_or_else(|| NuclideLoadError::WrongType {
                path: path.to_path_buf(),
                field: "Daughters[*]",
                actual: type_name(entry),
            })?;
        if arr.len() < 3 {
            return Err(NuclideLoadError::InvalidValue {
                path: path.to_path_buf(),
                field: "Daughters[*]",
                reason: format!("expected at least 3 elements, got {}", arr.len()),
            });
        }

        let decay_mode = arr[0]
            .as_str()
            .ok_or_else(|| NuclideLoadError::WrongType {
                path: path.to_path_buf(),
                field: "Daughters[*][0]",
                actual: type_name(&arr[0]),
            })?
            .to_string();

        // arr[0] = decay mode ("(B-)", "(alpha)", "EC", "IT", …) — unused for now.
        let daughter_name = arr[1].as_str().ok_or_else(|| NuclideLoadError::WrongType {
            path: path.to_path_buf(),
            field: "Daughters[*][1]",
            actual: type_name(&arr[1]),
        })?;
        let daughter_id =
            NuclideId::from_name(daughter_name).map_err(|_| NuclideLoadError::InvalidNuclide {
                path: path.to_path_buf(),
                id: daughter_name.to_string(),
            })?;
        let branching = arr[2].as_f64().ok_or_else(|| NuclideLoadError::WrongType {
            path: path.to_path_buf(),
            field: "Daughters[*][2]",
            actual: type_name(&arr[2]),
        })?;
        daughters.push((daughter_id, decay_mode, branching));
    }

    // Emissions may be absent, matching Python's `.get("Emissions", [])`.
    let emissions: Vec<Emission> = match data.get("Emissions") {
        Some(Value::Array(arr)) => arr
            .iter()
            .map(|e| parse_emission(e, nuclide_id, path))
            .collect::<NuclideLoadResult<_>>()?,
        Some(other) => {
            return Err(NuclideLoadError::WrongType {
                path: path.to_path_buf(),
                field: "Emissions",
                actual: type_name(other),
            });
        }
        None => Vec::new(),
    };

    Ok(Nuclide {
        nuclide: nuclide_id,
        element,
        daughters,
        half_life_s,
        specific_activity_bq_per_g,
        citation_ref,
        emissions,
    })
}

fn parse_emission(e: &Value, parent: NuclideId, path: &Path) -> NuclideLoadResult<Emission> {
    let energy = get_f64(e, "Energy (keV)", path)?;
    let energy_err = get_f64(e, "Energy error (keV)", path)?;
    let intensity = get_f64(e, "I (%)", path)?;
    let intensity_err = get_f64(e, "I error (%)", path)?;
    let raw_type = get_str(e, "Type", path)?;
    let raw_origin = get_str(e, "Origin", path)?;

    // Replicates `not np.isclose(energy, 511)`: if close, force Annihilation
    // type regardless of the declared one.
    let r#type = if is_annihilation_energy(energy) {
        Some(EmissionType::Annihilation)
    } else {
        parse_emission_type(raw_type)
    };

    Ok(Emission {
        // Matches Python: parent is always the source nuclide, even for the
        // 511-keV emission. `Emission::annihilation()` uses `None` instead,
        // but the loader follows the JSON's explicit association.
        parent_nuclide: Some(parent),
        energy_kev: Some(Quantity::new_scalar(energy, energy_err, kev(), None, None)),
        intensity_percent: Some(Quantity::new_scalar(
            intensity,
            intensity_err,
            percent(),
            None,
            None,
        )),
        r#type,
        origin: parse_emission_origin(raw_origin),
    })
}

/// `np.isclose(x, 511)` with default `rtol=1e-05, atol=1e-08`.
fn is_annihilation_energy(e: f64) -> bool {
    const RTOL: f64 = 1e-05;
    const ATOL: f64 = 1e-08;
    (e - 511.0).abs() <= RTOL.mul_add(511.0, ATOL)
}

fn parse_emission_type(s: &str) -> Option<EmissionType> {
    let lower = s.trim().to_ascii_lowercase();
    match lower.as_str() {
        "gamma" | "g" => Some(EmissionType::Gamma),
        "beta-" | "beta minus" | "b-" | "β-" | "negatron" => Some(EmissionType::BetaMinus),
        "beta+" | "beta plus" | "b+" | "β+" | "positron" => Some(EmissionType::BetaPlus),
        "alpha" | "a" | "α" => Some(EmissionType::Alpha),
        "x-ray" | "xray" | "x" => Some(EmissionType::XRay),
        "ce" | "conversion electron" => Some(EmissionType::ConversionElectron),
        "annih." | "annih" | "annihilation" => Some(EmissionType::Annihilation),
        // Unknown types map to `None`; extend this match as new labels appear.
        _ => None,
    }
}

fn parse_emission_origin(s: &str) -> Option<EmissionOrigin> {
    let lower = s.trim().to_ascii_lowercase();
    match lower.as_str() {
        "annih." | "annih" | "annihilation" => Some(EmissionOrigin::Annihilation),
        "decay" | "nuclear decay" => Some(EmissionOrigin::NuclearDecay),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// JSON helpers
// ---------------------------------------------------------------------------

fn get_str<'a>(value: &'a Value, field: &'static str, path: &Path) -> NuclideLoadResult<&'a str> {
    match value.get(field) {
        Some(Value::String(s)) => Ok(s.as_str()),
        Some(other) => Err(NuclideLoadError::WrongType {
            path: path.to_path_buf(),
            field,
            actual: type_name(other),
        }),
        None => Err(NuclideLoadError::MissingField {
            path: path.to_path_buf(),
            field,
        }),
    }
}

fn get_f64(value: &Value, field: &'static str, path: &Path) -> NuclideLoadResult<f64> {
    match value.get(field) {
        Some(Value::Number(n)) => n.as_f64().ok_or_else(|| NuclideLoadError::InvalidValue {
            path: path.to_path_buf(),
            field,
            reason: format!("number {n} is not representable as f64"),
        }),
        Some(other) => Err(NuclideLoadError::WrongType {
            path: path.to_path_buf(),
            field,
            actual: type_name(other),
        }),
        None => Err(NuclideLoadError::MissingField {
            path: path.to_path_buf(),
            field,
        }),
    }
}

fn get_pair(value: &Value, field: &'static str, path: &Path) -> NuclideLoadResult<(f64, f64)> {
    let arr = match value.get(field) {
        Some(Value::Array(a)) => a,
        Some(other) => {
            return Err(NuclideLoadError::WrongType {
                path: path.to_path_buf(),
                field,
                actual: type_name(other),
            });
        }
        None => {
            return Err(NuclideLoadError::MissingField {
                path: path.to_path_buf(),
                field,
            });
        }
    };
    if arr.len() != 2 {
        return Err(NuclideLoadError::InvalidValue {
            path: path.to_path_buf(),
            field,
            reason: format!("expected 2 elements, got {}", arr.len()),
        });
    }
    let mean = arr[0].as_f64().ok_or_else(|| NuclideLoadError::WrongType {
        path: path.to_path_buf(),
        field,
        actual: type_name(&arr[0]),
    })?;
    let std_dev = arr[1].as_f64().ok_or_else(|| NuclideLoadError::WrongType {
        path: path.to_path_buf(),
        field,
        actual: type_name(&arr[1]),
    })?;
    Ok((mean, std_dev))
}

fn missing_or_wrong(value: &Value, field: &'static str, path: &Path) -> NuclideLoadError {
    value.get(field).map_or_else(
        || NuclideLoadError::MissingField {
            path: path.to_path_buf(),
            field,
        },
        |other| NuclideLoadError::WrongType {
            path: path.to_path_buf(),
            field,
            actual: type_name(other),
        },
    )
}

const fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid_nuclide_json() -> Value {
        json!({
            "Nuclide": "Cs-137",
            "Element": "Cesium",
            "citation_ref": "ENSDF",
            "Half-life (s)": [9.49e8, 1.0e6],
            "Specific activity (Bq/g)": [3.21e12, 1.0e10],
            "Daughters": [["(B-)", "Ba-137m", 0.944]],
            "Emissions": [
                {
                    "Energy (keV)": 661.657,
                    "Energy error (keV)": 0.003,
                    "I (%)": 85.1,
                    "I error (%)": 0.2,
                    "Type": "gamma",
                    "Origin": "nuclear decay"
                },
                {
                    "Energy (keV)": 511.0,
                    "Energy error (keV)": 0.0,
                    "I (%)": 1.0,
                    "I error (%)": 0.1,
                    "Type": "gamma",
                    "Origin": "annih."
                },
                {
                    "Energy (keV)": 100.0,
                    "Energy error (keV)": 0.1,
                    "I (%)": 2.0,
                    "I error (%)": 0.1,
                    "Type": "beta-",
                    "Origin": "unknown"
                },
                {
                    "Energy (keV)": 200.0,
                    "Energy error (keV)": 0.1,
                    "I (%)": 2.0,
                    "I error (%)": 0.1,
                    "Type": "beta+",
                    "Origin": "decay"
                },
                {
                    "Energy (keV)": 300.0,
                    "Energy error (keV)": 0.1,
                    "I (%)": 2.0,
                    "I error (%)": 0.1,
                    "Type": "alpha",
                    "Origin": "decay"
                },
                {
                    "Energy (keV)": 32.0,
                    "Energy error (keV)": 0.1,
                    "I (%)": 2.0,
                    "I error (%)": 0.1,
                    "Type": "x-ray",
                    "Origin": "decay"
                },
                {
                    "Energy (keV)": 624.0,
                    "Energy error (keV)": 0.1,
                    "I (%)": 2.0,
                    "I error (%)": 0.1,
                    "Type": "ce",
                    "Origin": "decay"
                },
                {
                    "Energy (keV)": 400.0,
                    "Energy error (keV)": 0.1,
                    "I (%)": 2.0,
                    "I error (%)": 0.1,
                    "Type": "annihilation",
                    "Origin": "annihilation"
                },
                {
                    "Energy (keV)": 50.0,
                    "Energy error (keV)": 0.1,
                    "I (%)": 2.0,
                    "I error (%)": 0.1,
                    "Type": "mystery",
                    "Origin": "decay"
                }
            ]
        })
    }

    #[test]
    fn test_load_nuclide_data_and_file_errors() {
        let p = Path::new("test.json");
        let nuc = nuclide_from_json(&valid_nuclide_json(), p).unwrap();
        assert_eq!(nuc.emissions.len(), 9);
        assert_eq!(nuc.emissions[1].r#type, Some(EmissionType::Annihilation));
        assert_eq!(nuc.emissions[8].r#type, None);
        assert_eq!(nuc.emissions[2].origin, None);

        // Missing Emissions -> empty vec
        let mut no_em = valid_nuclide_json();
        no_em.as_object_mut().unwrap().remove("Emissions");
        assert!(nuclide_from_json(&no_em, p).unwrap().emissions.is_empty());

        // Real nuclide_data directory + glob error
        let all = load_nuclide_data("resources/nuclide_data/Cs-137.json").unwrap();
        assert_eq!(all.len(), 1);
        assert!(matches!(load_nuclide_data("[invalid"), Err(NuclideLoadError::Glob(_))));

        // File I/O and JSON parse errors
        let missing = Path::new("/tmp/does_not_exist_nuclide_xyz.json");
        assert!(matches!(nuclide_from_file(missing), Err(NuclideLoadError::Io { .. })));

        let dir = std::env::temp_dir().join("luracs_test_nuclide_loader");
        std::fs::create_dir_all(&dir).unwrap();
        let bad_json = dir.join("bad.json");
        std::fs::write(&bad_json, "{bad json").unwrap();
        assert!(matches!(nuclide_from_file(&bad_json), Err(NuclideLoadError::Json { .. })));
        drop(std::fs::remove_dir_all(&dir));
    }

    #[test]
    fn test_nuclide_from_json_validation_errors() {
        let p = Path::new("err.json");

        // Invalid nuclide & element
        let mut v = valid_nuclide_json();
        v["Nuclide"] = json!("NotANuclide-999");
        assert!(matches!(nuclide_from_json(&v, p), Err(NuclideLoadError::InvalidNuclide { .. })));

        let mut v = valid_nuclide_json();
        v["Element"] = json!("Unobtainium");
        assert!(matches!(nuclide_from_json(&v, p), Err(NuclideLoadError::InvalidElement { .. })));

        // Wrong type / missing fields
        let mut v = valid_nuclide_json();
        v["Nuclide"] = json!(123);
        assert!(matches!(nuclide_from_json(&v, p), Err(NuclideLoadError::WrongType { .. })));

        let mut v = valid_nuclide_json();
        v.as_object_mut().unwrap().remove("Nuclide");
        assert!(matches!(nuclide_from_json(&v, p), Err(NuclideLoadError::MissingField { .. })));

        // Daughters errors
        for bad_daughters in [
            json!("not-array"),
            json!(["not-entry-array"]),
            json!([["a", "b"]]),
            json!([[1, "Ba-137m", 0.5]]),
            json!([["B-", 2, 0.5]]),
            json!([["B-", "NoSuchNuc-999", 0.5]]),
            json!([["B-", "Ba-137m", "bad"]]),
        ] {
            let mut v = valid_nuclide_json();
            v["Daughters"] = bad_daughters;
            assert!(nuclide_from_json(&v, p).is_err());
        }
        let mut v = valid_nuclide_json();
        v.as_object_mut().unwrap().remove("Daughters");
        assert!(matches!(nuclide_from_json(&v, p), Err(NuclideLoadError::MissingField { .. })));

        // Emissions wrong type
        let mut v = valid_nuclide_json();
        v["Emissions"] = json!(true);
        assert!(matches!(nuclide_from_json(&v, p), Err(NuclideLoadError::WrongType { .. })));

        // get_f64 & get_pair errors
        for bad_hl in [json!(null), json!("str"), json!([1.0]), json!(["x", 1.0]), json!([1.0, false])] {
            let mut v = valid_nuclide_json();
            v["Half-life (s)"] = bad_hl;
            assert!(nuclide_from_json(&v, p).is_err());
        }
        let mut v = valid_nuclide_json();
        v.as_object_mut().unwrap().remove("Half-life (s)");
        assert!(nuclide_from_json(&v, p).is_err());

        let mut v = valid_nuclide_json();
        v["Emissions"][0]["Energy (keV)"] = json!({});
        assert!(nuclide_from_json(&v, p).is_err());
        let mut v = valid_nuclide_json();
        v["Emissions"][0].as_object_mut().unwrap().remove("Energy (keV)");
        assert!(nuclide_from_json(&v, p).is_err());
    }
}
