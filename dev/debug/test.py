import numpy as np

from interpolator import rebin_histogram


# Original histogram bin edges
energy_axis = np.linspace(0, 100, 101)   # 100 bins, width = 1

# Create a synthetic spectrum:
# - Gaussian peak around 40
# - Smaller peak around 75
# - Background noise
centers = 0.5 * (energy_axis[:-1] + energy_axis[1:])

counts = (
    500 * np.exp(-0.5 * ((centers - 40) / 5)**2)
    + 200 * np.exp(-0.5 * ((centers - 75) / 3)**2)
    + 20
)

# Add Poisson counting noise
rng = np.random.default_rng(42)
counts = rng.poisson(counts).astype(np.float64)

print("Original counts:", counts.sum())

# New axis: twice the resolution
new_energy_axis = np.linspace(0, 100, 51)  # 200 bins

# Call your Cython function
new_counts = rebin_histogram(
    energy_axis,
    counts,
    new_energy_axis,
)

print("Rebinned counts:", new_counts.round().sum())
print("Difference:", new_counts.round().sum() - counts.sum())

print(counts)
print(new_counts.round())
