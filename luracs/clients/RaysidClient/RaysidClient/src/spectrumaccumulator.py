import time
import numpy as np
from .data_classes import SpectrumResult


class SpectrumAccumulator:
    """Class to handle spectrum information. Both channel counts and meta data."""

    def __init__(self, n_channels=1800):
        self.n = n_channels
        self.reset()

    def reset(self):
        self.counts = self.uptime = self.energy = self.high_energy_counts = 0

        self.spectrum = np.zeros(self.n, dtype=np.float64)

    def insert(self, start_ch, end_ch, spectrum_chunk):
        """Insert a decoded spectrum chunk"""

        if not ((start_ch == 0) and (end_ch == 0)):
            self.spectrum[start_ch:end_ch] = spectrum_chunk[start_ch:end_ch]

    def update_meta(self, counts, uptime, energy, high_energy_counts):
        self.counts = counts
        self.uptime = uptime
        self.energy = energy
        self.high_energy_counts = high_energy_counts

    def snapshot(self):
        return SpectrumResult(
            spectrum=self.spectrum.copy(),
            timestamp=time.time(),
            counts=self.counts,
            uptime=self.uptime,
            energy=self.energy,
            high_E_counts=self.high_energy_counts,
        )
