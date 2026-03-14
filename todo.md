# Todo
## Program structure

## Core
- [ ] Restructure inclusion of tabs in a seperate file
- [ ] Clean up main
- [ ] Fix RunManager
- [ ] Settings
  - [ ] Load and dump json files
  - [ ] Apperance
  - [ ] Program state
## Gamma tools

- [ ] Radionuclides
  - [ ] Plot lines in the spectrumplot based on yield
  - [ ] Radionuclide data should be hardcoded in a .py file, not loaded
- [ ] Implement basic ufloat to avoid the uncertainties dependency
- [ ] Calibration
- [ ] Activity Calculation
- [ ] Photopeak
- [ ] Efficiency

## File IO
- [ ] XML/n42
- [ ] csv (export)
- [ ] spe (read)
- [ ] tke (read)
- [ ] rois, use json format

## Time based measurements (low prio)
- [ ] Doserate and CPS log (sqlite3)
- [ ] Spectrum log (sqlite3)
- [ ] Waterfall diagram
- [ ] review tools
- [ ] Fixed length measurement


## Known issues

- Theme looks bad on windows
- PyUSB drivers work poorly on windows
- Current values are not removed with device
- Spectrum and current values are not synced
