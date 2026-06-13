# LuRaCs - *Lund Radiation analysis Computer software*

![License](https://img.shields.io/badge/license-GPL--3.0-blue.svg) ![Docs License](https://img.shields.io/badge/docs-CC_BY_4.0-lightgrey.svg) ![Python](https://img.shields.io/badge/python-3.10+-blue.svg) ![Qt](https://img.shields.io/badge/PySide6-Qt6-green.svg) ![MapLibre](https://img.shields.io/badge/MapLibre-GL_JS-blue.svg)

LuRaCs is a free, open-source application designed for the measurement and analysis of nuclear radiation. While its functionality ranges from basic to advanced radiation analysis, with primary focus is on gamma spectrometry. Built with Qt and primarily written in Python, the software is designed with extensibility in mind, enabling support for a variety of detector systems.

- [LuRaCs - *Lund Radiation analysis Computer software*](#luracs---lund-radiation-analysis-computer-software)
  - [Features](#features)
    - [Spectrum View](#spectrum-view)
    - [Analysis Tools](#analysis-tools)
    - [Time Series Measurements](#time-series-measurements)
    - [Area Mapping](#area-mapping)
    - [CLI](#cli)
    - [Implemented Instruments](#implemented-instruments)
  - [Installation](#installation)
  - [Documentation](#documentation)
  - [Licence](#licence)

## Features
- Simultaneous measurements with several instruments
- Rich data export
- ROI selection with peak fitting, automatic update during measurement
- Calibration and calculation of detector characteristics
- Control trough a command line interface
- Time series and spectrogram measurements
- Area Mapping with a generic GPS connected through USB
### Spectrum View
The spectrum view offers a count histogram retrieved from an instrument or loaded from a saved file. *Regions Of Interest* (ROI) can be defined in which a Gaussian peak can be fitted and the result from the fit used for further analysis.
![spect_view](luracs/resources/docs/documentation/Interface/imgs/spect_view.webp)


### Analysis Tools
LuRaCs offers a set of tools for analysing a measured *spectrum*. A spectrum can be calibrated to known emissions and instrument characteristics energy resolution and intrinsic efficiency can 
![analysis_view](luracs/resources/docs/documentation/Interface/imgs/analysis.webp)
### Time Series Measurements
When an instrument is connected, reading can be measured in a time series in the form of a *spectrogram*. A spectrogram saves the mean values of the count- and dose rate during each save interval as well as the collected spectrum.
![timeseries_view](luracs/resources/docs/documentation/Interface/imgs/timeseries_view.webp)
### Area Mapping
By connecting an external GPS through USB area mapping can be carried out by extending the normal spectrogram measurements. Maps can be requested from an internet URL or provided from a downloaded file for offline use.
### CLI
While the GUI provides to most common interface, LuRaCs also supports control through a Command Line Interface. Enabling efficient use over an ssh tunnel for connecting the remotely connected instruments, for instance used with a *RaspberryPi*.

The CLI is started by running with the `--headless` flag.

![cli_img](luracs/resources/docs/documentation/Interface/imgs/cli.webp) ![cli_spect](luracs/resources/docs/documentation/Interface/imgs/cli_spect.webp)


### Implemented Instruments
Currently, drivers for the following instruments are included by default
- [RadiaCode-1xx series](https://www.radiacode.com/100-series)
- [Raysid](https://raysid.com/)

## Installation
Requires Python 3.10 or newer.
Clone the repository and install the desired version:

```shell
git clone --recursive https://github.com/EBELU/LuRaCs.git
cd LuRaCs/

# Full version (includes all optional dependencies)
pip install ".[full]"

# Lightweight version (core dependencies only)
pip install .
```
## Documentation
LuRaCs documentation can be found [here](luracs/resources/docs/documentation/Welcome.md) or under the *Help*-tab in the open program. 

## Licence
The LuRaCs source code is licensed under the GPL-3.0 license.

The LuRaCs documentation and images are licensed under the CC BY-SA 4.0 license.