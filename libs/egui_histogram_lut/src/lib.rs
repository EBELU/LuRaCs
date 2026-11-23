//! PyQtGraph-faithful `HistogramLUTWidget` and `GradientEditorItem` for `egui`.

pub mod gradient;
pub mod widget;

pub use gradient::{ColorMode, Gradient, GradientPreset, GradientTick};
pub use widget::{
    HistogramData, HistogramLutResponse, HistogramLutState, HistogramLutWidget, Orientation,
};
