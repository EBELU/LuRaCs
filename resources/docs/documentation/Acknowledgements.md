# Acknowledgements

This application is written in Rust and uses the following open-source crates:

| Crate | Purpose | License |
|---|---|---|
| eframe / egui | Immediate-mode GUI framework | MIT OR Apache-2.0 |
| egui_commonmark | Markdown viewer for egui | MIT OR Apache-2.0 |
| walkers | Slippy map widget (MVT / PMTiles) | MIT |
| tokio | Asynchronous runtime | MIT |
| ndarray | N-dimensional arrays | MIT OR Apache-2.0 |
| nalgebra | Linear algebra & fitting | Apache-2.0 |
| physure | Physical quantities & uncertainty propagation | MIT OR Apache-2.0 |
| nucleide-nuclei | Nuclide identifiers & decay data | MIT OR Apache-2.0 |
| mendeleev | Chemical element data | MIT |
| btleplug | Cross-platform Bluetooth Low Energy | MIT OR Apache-2.0 OR BSD-3-Clause |
| nusb / rusb | USB device enumeration and I/O | MIT OR Apache-2.0 |
| serialport | Serial port communication (GPS) | MPL-2.0 |
| reqwest / rustls | HTTP & TLS client | MIT OR Apache-2.0 |
| rusqlite | Embedded SQLite database | MIT |
| rust_xlsxwriter | Excel `.xlsx` export | MIT OR Apache-2.0 |
| quick-xml / roxmltree | XML parsing and N42 generation | MIT / MIT OR Apache-2.0 |
| rfd | Native file dialogs | MIT |
| serde / serde_json / toml / csv | Serialization & data formats | MIT OR Apache-2.0 |
| rayon | Data parallelism | MIT OR Apache-2.0 |
| zip / flate2 | Archive & compression | MIT OR Apache-2.0 |

It also includes the workspace crates `rsplot` (GPU-accelerated `wgpu` plotting) and `egui_histogram_lut`.

We gratefully acknowledge the maintainers and contributors of these projects.

The following projects inspired or provided protocol/format references used in this project:

- [pmtiles](https://github.com/protomaps/PMTiles)
- [radiacode](https://github.com/cdump/radiacode/tree/master)
- [maplibre](https://github.com/maplibre/maplibre-gl-js)

## Colour Scheme and Icons
This application uses the [Catppuccin](https://catppuccin.com/) colour scheme and [Material Design Icons](https://pictogrammers.com/library/mdi/) (Apache 2.0).

## Project License

This application itself is licensed under the GNU General Public License v3.0 (GPL-3.0).
