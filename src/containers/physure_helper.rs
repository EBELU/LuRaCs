//! Helpers for working with `physure_core::Quantity`: unit constructors and
//! serde bridges (physure ships no serde impls, so a `Quantity` is serialised
//! as `{ "mean": …, "std_dev": … }`).

use physure_core::RationalUnit;

// ---------------------------------------------------------------------------
// Unit constructors
// ---------------------------------------------------------------------------

pub fn dimensionless() -> RationalUnit {
    RationalUnit::new_from_dimensions([])
}

pub fn cm() -> RationalUnit {
    RationalUnit::new_from_dimensions([("cm".to_string(), (1, 1))])
}

pub fn kev() -> RationalUnit {
    RationalUnit::new_from_dimensions([("keV".to_string(), (1, 1))])
}

pub fn percent() -> RationalUnit {
    RationalUnit::new_from_dimensions([("%".to_string(), (1, 1))])
}

pub fn seconds() -> RationalUnit {
    RationalUnit::new_from_dimensions([("s".to_string(), (1, 1))])
}

pub fn bq_per_g() -> RationalUnit {
    RationalUnit::new_from_dimensions([("Bq".to_string(), (1, 1)), ("g".to_string(), (-1, 1))])
}

// ---------------------------------------------------------------------------
// Serde representation
// ---------------------------------------------------------------------------

pub mod quantity_repr {
    use physure_core::quantity::Quantity;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize)]
    pub struct Repr {
        pub mean: f64,
        pub std_dev: f64,
    }

    impl Repr {
        pub fn from_quantity(q: &Quantity) -> Self {
            Self {
                mean: q.value.mean(),
                std_dev: q.value.std_dev(),
            }
        }

        pub fn into_quantity(self, unit: physure_core::units::rational::RationalUnit) -> Quantity {
            Quantity::new_scalar(self.mean, self.std_dev, unit, None, None)
        }
    }
}

// ---------------------------------------------------------------------------
// Serde bridges: bare `Quantity` (non-optional field)
// ---------------------------------------------------------------------------

pub mod quantity_serde {
    macro_rules! module_for_unit {
        ($name:ident, $unit_fn:path) => {
            pub mod $name {
                use physure_core::quantity::Quantity;
                use serde::{Deserialize, Deserializer, Serialize, Serializer};

                pub fn serialize<S>(v: &Quantity, s: S) -> Result<S::Ok, S::Error>
                where
                    S: Serializer,
                {
                    super::super::quantity_repr::Repr::from_quantity(v).serialize(s)
                }

                pub fn deserialize<'de, D>(d: D) -> Result<Quantity, D::Error>
                where
                    D: Deserializer<'de>,
                {
                    let r = super::super::quantity_repr::Repr::deserialize(d)?;
                    Ok(r.into_quantity($unit_fn()))
                }
            }
        };
    }

    module_for_unit!(seconds, super::super::seconds);
    module_for_unit!(bq_per_g, super::super::bq_per_g);
}

// ---------------------------------------------------------------------------
// Serde bridges: `Option<Quantity>` (single, optional)
// ---------------------------------------------------------------------------

pub mod opt_quantity_serde {
    macro_rules! module_for_unit {
        ($name:ident, $unit_fn:path) => {
            pub mod $name {
                use physure_core::quantity::Quantity;
                use serde::{Deserialize, Deserializer, Serialize, Serializer};

                pub fn serialize<S>(v: &Option<Quantity>, s: S) -> Result<S::Ok, S::Error>
                where
                    S: Serializer,
                {
                    match v {
                        Some(q) => super::super::quantity_repr::Repr::from_quantity(q).serialize(s),
                        None => s.serialize_none(),
                    }
                }

                pub fn deserialize<'de, D>(d: D) -> Result<Option<Quantity>, D::Error>
                where
                    D: Deserializer<'de>,
                {
                    let opt = Option::<super::super::quantity_repr::Repr>::deserialize(d)?;
                    Ok(opt.map(|r| r.into_quantity($unit_fn())))
                }
            }
        };
    }

    module_for_unit!(kev, super::super::kev);
    module_for_unit!(percent, super::super::percent);
}

// ---------------------------------------------------------------------------
// Serde bridges: `Option<Vec<Quantity>>`
// ---------------------------------------------------------------------------

pub mod opt_vec_quantity_serde {
    use physure_core::quantity::Quantity;
    use serde::{Deserialize, Deserializer, Serializer};

    pub mod cm {
        use super::*;
        pub fn serialize<S>(v: &Option<Vec<Quantity>>, s: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            super::inner::serialize(v, s)
        }
        pub fn deserialize<'de, D>(d: D) -> Result<Option<Vec<Quantity>>, D::Error>
        where
            D: Deserializer<'de>,
        {
            super::inner::deserialize(d, super::super::cm)
        }
    }

    pub mod dimensionless {
        use super::*;
        pub fn serialize<S>(v: &Option<Vec<Quantity>>, s: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            super::inner::serialize(v, s)
        }
        pub fn deserialize<'de, D>(d: D) -> Result<Option<Vec<Quantity>>, D::Error>
        where
            D: Deserializer<'de>,
        {
            super::inner::deserialize(d, super::super::dimensionless)
        }
    }

    pub mod inner {
        use physure_core::quantity::Quantity;
        use physure_core::units::rational::RationalUnit;
        use serde::{Deserialize, Deserializer, Serializer};

        pub fn serialize<S>(v: &Option<Vec<Quantity>>, s: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            match v {
                Some(v) => {
                    let reprs: Vec<super::super::quantity_repr::Repr> = v
                        .iter()
                        .map(super::super::quantity_repr::Repr::from_quantity)
                        .collect();
                    serde::Serialize::serialize(&reprs, s)
                }
                None => s.serialize_none(),
            }
        }

        pub fn deserialize<'de, D>(
            d: D,
            unit_fn: fn() -> RationalUnit,
        ) -> Result<Option<Vec<Quantity>>, D::Error>
        where
            D: Deserializer<'de>,
        {
            let opt: Option<Vec<super::super::quantity_repr::Repr>> = Option::deserialize(d)?;
            Ok(opt.map(|v| {
                let u = unit_fn();
                v.into_iter().map(|r| r.into_quantity(u.clone())).collect()
            }))
        }
    }
}
