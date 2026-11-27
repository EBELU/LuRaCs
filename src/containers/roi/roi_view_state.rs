// src/roi_view_state.rs

use crate::containers::{nuclide::Emission, roi::Roi};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Lower,
    Upper,
}

#[derive(Debug, Clone, Default)]
pub enum RoiInteraction {
    #[default]
    Idle,
    /// User is dragging one edge of a specific ROI.
    DraggingEdge { tag: String, edge: Edge },
    /// User is dragging the whole region (both bounds together).
    DraggingRegion {
        tag: String,
        grab_x: f64,
        initial_bound: (f64, f64),
    },
    /// Modal editor is open for this ROI.
    Editing { tag: String },
}

impl RoiInteraction {
    pub const fn is_active(&self) -> bool {
        !matches!(self, Self::Idle)
    }
}

/// UI-only flags. Model state lives in `Roi`.
#[derive(Debug, Clone, Default)]
pub struct RoiViewState {
    pub interaction: RoiInteraction,
    pub selected: Option<String>,
    pub edit_draft: Option<RoiEditDraft>,
}

/// A copy of the editable fields, used while a modal is open.
#[derive(Debug, Clone)]
pub struct RoiEditDraft {
    pub tag: String,
    pub alias: String,
    pub roi_bound: (f64, f64),
    pub fit_type: super::roi_types::FitType,
    pub bkg_type: super::roi_types::BkgType,
    pub bkg_est_channels: i32,
    pub merge: bool,
    pub poisson_weights: bool,
    pub emission: Option<Emission>,
}

impl RoiEditDraft {
    pub fn from_roi(r: &Roi) -> Self {
        Self {
            tag: r.tag.clone(),
            alias: r.alias.clone(),
            roi_bound: r.roi_bound,
            fit_type: r.fit_type,
            bkg_type: r.bkg_type,
            bkg_est_channels: r.bkg_est_channels,
            merge: r.merge,
            poisson_weights: r.poisson_weights,
            emission: r.emission.clone(),
        }
    }

    pub fn apply_to(self, r: &mut Roi) {
        r.alias = self.alias;
        r.roi_bound = self.roi_bound;
        r.fit_type = self.fit_type;
        r.bkg_type = self.bkg_type;
        r.bkg_est_channels = self.bkg_est_channels;
        r.merge = self.merge;
        r.poisson_weights = self.poisson_weights;
        r.emission = self.emission;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::containers::roi::roi_types::{BkgType, FitType};

    #[test]
    fn test_roi_interaction_and_edit_draft() {
        assert!(!RoiInteraction::Idle.is_active());
        assert!(
            RoiInteraction::DraggingEdge {
                tag: "r1".to_string(),
                edge: Edge::Lower,
            }
            .is_active()
        );
        assert!(
            RoiInteraction::DraggingRegion {
                tag: "r1".to_string(),
                grab_x: 100.0,
                initial_bound: (90.0, 110.0),
            }
            .is_active()
        );
        assert!(
            RoiInteraction::Editing {
                tag: "r1".to_string(),
            }
            .is_active()
        );

        let mut roi = Roi {
            tag: "r1".to_string(),
            alias: "Peak1".to_string(),
            spectrum: "spec1".to_string(),
            roi_bound: (650.0, 675.0),
            region_bound: (620.0, 700.0),
            fit_type: FitType::Gaussian,
            bkg_type: BkgType::Linear,
            bkg_est_channels: 3,
            fit: None,
            roi_counts: 100.0,
            live_time: 10.0,
            emission: None,
            meta: serde_json::Map::new(),
            merge: false,
            movable: true,
            poisson_weights: true,
            owner_spectrum: None,
        };

        let mut draft = RoiEditDraft::from_roi(&roi);
        assert_eq!(draft.tag, "r1");
        draft.alias = "Cs-137".to_string();
        draft.roi_bound = (655.0, 670.0);
        draft.fit_type = FitType::None;
        draft.bkg_type = BkgType::Quadratic;
        draft.bkg_est_channels = 5;
        draft.merge = true;
        draft.poisson_weights = false;
        draft.apply_to(&mut roi);

        assert_eq!(roi.alias, "Cs-137");
        assert_eq!(roi.roi_bound, (655.0, 670.0));
        assert_eq!(roi.fit_type, FitType::None);
        assert_eq!(roi.bkg_type, BkgType::Quadratic);
        assert_eq!(roi.bkg_est_channels, 5);
        assert!(roi.merge);
        assert!(!roi.poisson_weights);
    }
}
