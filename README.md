# LuRaCs - *Lund Radiation analysis Computer software*
![License](https://img.shields.io/badge/license-GPL--3.0-blue.svg) [![Docs: CC BY-SA 4.0](https://img.shields.io/badge/docs-CC_BY_SA_4.0-lightgrey.svg)](https://creativecommons.org/licenses/by-sa/4.0/) ![Python](https://img.shields.io/badge/python-3.10+-blue.svg) ![Qt](https://img.shields.io/badge/PySide6-Qt6-green.svg) ![MapLibre](https://img.shields.io/badge/MapLibre-GL_JS-blue.svg)

![Banner](luracs/resources/themes/icons/splash_base.png)

Copyright (C) 2026 Erik Ewald

LuRaCs is a free, open-source application designed for the measurement and analysis of nuclear radiation. While its functionality ranges from basic to advanced radiation analysis, with primary focus is on gamma spectrometry. Built with Qt and primarily written in Python, the software is designed with extensibility in mind, enabling support for a variety of detector systems.

- [LuRaCs - *Lund Radiation analysis Computer software*](#luracs---lund-radiation-analysis-computer-software)
  - [Features](#features)
    - [Spectrum View](#spectrum-view)
    - [Analysis Tools](#analysis-tools)
    - [Time Series Measurements](#time-series-measurements)
    - [Area Mapping](#area-mapping)
    - [Timed Measurements](#timed-measurements)
    - [CLI](#cli)
    - [Implemented Detectors](#implemented-detectors)
  - [Installation](#installation)
  - [Documentation](#documentation)
  - [Licence](#licence)
  - [Third-party assets](#third-party-assets)

## Features
- Simultaneous measurements with multiple instruments
- Data export to common formats (.n42, .csv, .xlsx)
- ROI selection with peak fitting and automatic updates during measurement
- Calibration and determination of detector characteristics
- Spectrum deconvolution using ML-EM
- Nuclide data with reference emission lines
- Command line interface for remote control and automation
- Time-series and spectrogram-based measurements
- Area mapping using a generic USB-connected GPS device
### Spectrum View
The spectrum view shows a count histogram retrieved from an instrument or loaded from a file. *Regions Of Interest* (ROI) can be defined in which a Gaussian peak can be fitted and the result from the fit used in analysis tools.
![spect_view](dev/GitHub_spectrum.png)
### Analysis Tools
LuRaCs provides a comprehensive set of tools for analysing measured spectra. Spectra can be calibrated using known emission lines, and key instrument characteristics such as energy resolution and intrinsic efficiency can be determined, visualized, and stored for future use.

If your primary goal is the analysis of previously acquired spectra, we recommend using [InterSpec](https://sandialabs.github.io/InterSpec/), which offers broader compatibility with a wide range of spectrum file formats and analysis workflows.
![analysis_view](luracs/resources/docs/imgs/analysis.webp)

LuRaCs also implements a deconvolution tool that can enhance a spectrum using a detector dependent response matrix.       
![deconv_view](dev/GitHub_deconv.png)
### Time Series Measurements
When an instrument is connected, measurements can be recorded as a time series in the form of a *spectrogram*. A spectrogram stores the average count rate and dose rate for each acquisition interval, together with the corresponding accumulated spectrum. This enables both temporal trend analysis and detailed examination of the spectral data collected throughout the measurement period.
![timeseries_view](luracs/resources/docs/imgs/timeseries_view.webp)
### Area Mapping
By connecting an external GPS device via USB, area mapping can be performed by extending standard spectrogram measurements with positional data. Maps can be retrieved from an online source via URL or loaded from a locally stored file for offline use. Supports export of mapped data to GeoJSON for use in common GIS applications like QGis.
![map_img](dev/GitHub_map.png)
### Timed Measurements
Spectra and rates can be measured with a timer for standardized protocols with a simple and intuitive  interface. The measurements can the be exported as reports containing both human readable text and computer readable data for importing the report to other spectrum analysis tools.
![measure_img](dev/GitHub_measure.png)
### CLI
While the graphical user interface provides the primary means of interaction, LuRaCs also supports control via a Command Line Interface. This enables efficient remote operation, for example over an SSH connection to remotely deployed instruments such as a *Raspberry Pi*.

The CLI is started by running with the `--headless` flag.

![cli_img](luracs/resources/docs/imgs/cli.webp) ![cli_spect](luracs/resources/docs/imgs/cli_spect.webp)


### Implemented Detectors
Currently, drivers for the following detectors are included by default
- [RadiaCode-1xx series & Zero](https://www.radiacode.com/100-series) (USB, BLE)
- [Raysid](https://raysid.com/) (BLE)
- Oretc digiBase (USB)
- Ortec digiDart (USB)
- Ortec Detecive X (NETWORK)
- Flir Identifinder R400 (NETWORK)

## Installation
LuRaCs can be installed from PyPi using the following command.
```shell
pip install luracs
```
Requires Python 3.10, 3.11, 3.12 or 3.13 and a C/C++-compiler.
When installing from source, clone the repository and install using pip:

```shell
git clone --recursive https://github.com/EBELU/LuRaCs.git
cd LuRaCs/

pip install .[dev]
```
## Documentation
LuRaCs documentation can be found [here](luracs/resources/docs/documentation/Welcome.md) or under the *Help*-tab in the open program. 

The full reference manual can be found [here](https://github.com/EBELU/LuRaCs-UserManual)

## Licence
The LuRaCs source code is licensed under the GPL-3.0 license.

The LuRaCs documentation and images are licensed under the CC BY-SA 4.0 license.

## Third-party assets
Copies of third partly licences and notices are packaged in `/licenes`.




