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
    DraggingRegion { tag: String, grab_x: f64 },
    /// Modal editor is open for this ROI.
    Editing { tag: String },
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
    pub fit_type: super::roi::FitType,
    pub bkg_type: super::roi::BkgType,
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
