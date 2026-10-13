#!/home/eewa/anaconda3/bin/python3
# -*- coding: utf-8 -*-
"""
Created on Sun Oct 11 19:49:35 2026

@author: Erik Ewald
"""

import os
os.chdir(os.path.dirname(os.path.realpath(__file__)))
import numpy as np
import matplotlib.pyplot as plt
import pandas as pd

Lu = np.insert(pd.read_csv("/home/eewa/Documents/git/MySpect/dev/poster_spectra/Lu-177.csv").to_numpy().T, 0, 0, axis=1)
Cs = np.insert(pd.read_csv("/home/eewa/Documents/git/MySpect/dev/poster_spectra/Cs-137.csv").to_numpy().T, 0, 0, axis=1)
Ra = np.insert(pd.read_csv("/home/eewa/Documents/git/MySpect/dev/poster_spectra/Ra-226.csv").to_numpy().T, 0, 0, axis=1)

Lu[1] = Lu[1] / np.max(Lu[1])
Cs[1] = Cs[1] / np.max(Cs[1])
Ra[1] = Ra[1] / np.max(Ra[1])

# Typography and rendering
plt.rcParams.update({
    "font.family": "DejaVu Sans",
    "font.size": 12,
    "figure.dpi": 160,
    "savefig.dpi": 300,
    "text.antialiased": True,
})

# Catppuccin-inspired palette
bg = "#1e1e2e"
fg = "#cdd6f4"
muted = "#6c7086"
grid = "#45475a"

clrs = ["#1e66f5", "#df8e1d", "#40a02b"]  # Ra, Cs, Lu
labels = ["Lu-177", "Cs-137", "Ra-226"]

cutoff = 650

fig, ax = plt.subplots(figsize=(9, 4), facecolor=bg)
ax.set_facecolor(bg)

# Frame
for spine in ax.spines.values():
    spine.set_color(grid)
    spine.set_linewidth(0.9)

# Ticks and subtle grid
ax.tick_params(axis="x", colors=fg, length=0, pad=9)
ax.tick_params(axis="y", left=False, labelleft=False)
ax.grid(axis="x", color=grid, linewidth=0.6, alpha=0.35)
ax.set_axisbelow(True)

# Axis label
ax.set_xlabel("Energy (/keV)", color=fg, labelpad=12, fontsize=13)

# Smoothing / low-energy shaping
xaxis = np.arange(1025)
slope = np.polyval([1, 4], xaxis)

slop_cut = 13
slope = slope / np.max(slope[:slop_cut])

smoother = np.ones_like(slope)
smoother[:slop_cut] = slope[:slop_cut]

# Plot spectra
for clr, label, (x, y) in zip(clrs, labels, [Ra, Cs, Lu]):
    y_plot = (y * smoother)[:cutoff]
    x_plot = x[:cutoff]

    ax.fill_between(
        x_plot, y_plot,
        color=clr,
        alpha=0.14,
        linewidth=0,
    )

    ax.step(
        x_plot, y_plot,
        color=clr,
        linewidth=1.5,
        label=label,
        solid_capstyle="round",
        antialiased=True,
    )

# # Legend
# leg = ax.legend(
#     loc="upper right",
#     frameon=True,
#     facecolor=bg,
#     edgecolor=grid,
#     framealpha=0.95,
#     fontsize=11,
#     labelcolor=fg,
#     handlelength=2.2,
#     borderpad=0.8,
# )

# for line in leg.get_lines():
#     line.set_linewidth(2.5)

# Clean margins
ax.margins(x=0.01)
fig.tight_layout(pad=1.5)

# Export for poster use
fig.savefig(
    "spectra_poster.png",
    dpi=300,
    facecolor=fig.get_facecolor(),
    bbox_inches="tight",
)

plt.show()