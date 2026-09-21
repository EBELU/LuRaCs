
import numpy as np

from luracs.containers.roi_classes import Fit
from luracs.utils.numerics import (
    curve_fit,
    multi_gaussian,
    multi_gaussian_jacobian,
    poisson_weights,
)


def fit_gaussians(
    x_axis: np.ndarray,
    y_axis: np.ndarray,
    bounds: tuple,
    fit_type: str,
    use_poisson_weights: bool,
    weigh_cov_chi2: bool,
    bkg_type: str,
    bkg_est_channels: int,
) -> tuple[list[Fit], bool]:
    "Fit peaks to roi_group"
    region_min, region_max = np.min(bounds), np.max(bounds)
    region = slice(np.searchsorted(x_axis, region_min), np.searchsorted(x_axis, region_max))
    x_region = x_axis[region].copy().astype(float)
    y_region = y_axis[region].copy().astype(float)

    p0 = []
    for b in bounds:
        lower, upper = np.min(b), np.max(b)

        # mask for this peak window
        peak_mask = slice(np.searchsorted(x_region, lower), np.searchsorted(x_region, upper))

        x_peak = x_region[peak_mask]
        y_peak = y_region[peak_mask]

        if len(x_peak) == 0:
            continue  # skip empty regions

        # Initial guesses
        A0 = np.max(y_peak)
        mu0 = x_peak[np.argmax(y_peak)]
        s0 = (upper - lower) / 6.0

        p0.extend([A0, mu0, s0])

    p0s = np.asarray(p0)
    if np.any(p0s < 1e-8):
        return None, False

    # --- Fit the background as a polynomial ---
    if bkg_type != "None":
        i_low = np.searchsorted(x_axis, region_min)
        i_high = np.searchsorted(x_axis, region_max)

        bkg_extention_lower = i_low + bkg_est_channels
        bkg_extention_lower = max(bkg_extention_lower, 0)

        lower_bkg_points_x = x_axis[i_low:bkg_extention_lower]
        lower_bkg_points_y = y_axis[i_low:bkg_extention_lower]

        bkg_extention_upper = i_high - bkg_est_channels
        bkg_extention_upper = min(bkg_extention_upper, len(x_axis) - 1)

        upper_bkg_points_x = x_axis[bkg_extention_upper:i_high]
        upper_bkg_points_y = y_axis[bkg_extention_upper:i_high]

        if bkg_type == "Linear":
            poly_order = 1
        elif bkg_type == "Quadratic":
            poly_order = 2
        else:
            raise ValueError(f"Invald background type {bkg_type}")
        
        x_points = np.concatenate((lower_bkg_points_x, upper_bkg_points_x))
        y_points = np.concatenate((lower_bkg_points_y, upper_bkg_points_y))
        
        
        background_std = np.sqrt(np.maximum(y_points, 1.0))
        background_weights = 1.0 / background_std

        # Perform the polynomial fit with weights based on the poisson distribution of each channel
        # cov is not scaled with chi2 since the uncertainty is known is does not need to be estimated
        # from generic noise
        bkg_fit, poly_cov = np.polyfit(
            x_points,
            y_points,
            poly_order,
            w=background_weights,
            cov="unscaled"
        )
        
        
        # Design matrix for polynomial evaluation
        X_region = np.vander(x_region, N=poly_order + 1)
        bkg_cov = X_region @ poly_cov @ X_region.T
        # Variance of fitted background at each x
        # bkg_var = np.einsum(
        #     "ij,jk,ik->i",
        #     X_region,
        #     poly_cov,
        #     X_region,
        # )

        # bkg_std = np.sqrt(np.maximum(bkg_var, 0))
        # measurement_cov = np.diag(
        #     np.maximum(y_region, 1.0)
        # )

        bkg_channel_counts = np.polyval(bkg_fit, x_region)

        y_region -= bkg_channel_counts

    else:
        bkg_fit = None

    # Vectorize!
    p0 = p0s.flatten()

    # If you want to emulate Poission maximum likelihood
    weight = None
    if use_poisson_weights:
        weight = poisson_weights

    # Perform fit
    fits, cov, converged = curve_fit(
        multi_gaussian,
        x_region,
        y_region,
        p0,
        jac=multi_gaussian_jacobian,
        weight_fn=weight,
        weight_cov_chi2=weigh_cov_chi2,
    )

    # Sanity check
    if (
        np.any(fits > 1e12)
        or np.any(np.isnan(fits))
        or np.any(np.diag(cov) < 0)
        or np.any(np.sqrt(np.diag(cov)) > 1e12)
    ):
        return None, False
    
    # Propagate correlated background-model uncertainty
    # through the local least-squares parameter sensitivity
    if bkg_type != "None":
        J = multi_gaussian_jacobian(
            x_region,
            fits,
        )
        
        JtJ_inv = np.linalg.pinv(J.T @ J)

        # Sandwich covariance propagation (lmao)
        cov_background = (
            JtJ_inv
            @ J.T
            @ bkg_cov
            @ J
            @ JtJ_inv
        )

        param_cov = cov + cov_background
    else:
        param_cov = cov

    print("Measurement errors:")
    print(np.sqrt(np.diag(cov)))

    print("Background errors:")
    print(np.sqrt(np.diag(cov_background)))

    print("Total errors:")
    print(np.sqrt(np.diag(param_cov)))


    # Turn the vectorized array back to one list per peak
    fits = fits.reshape(-1, 3)
    errs = np.sqrt(np.diag(param_cov)).reshape(-1, 3)

    # --- Evaluation ---
    results = []
    for b, fit, err in zip(bounds, fits, errs):
        lower, upper = np.min(b), np.max(b)
        
        i0 = np.searchsorted(x_axis, lower)
        i1 = np.searchsorted(x_axis, upper)

        x_peak = x_axis[i0:i1]
        y_peak = y_axis[i0:i1]

        G = np.sum(y_peak)
        B = np.sum(np.polyval(bkg_fit, x_peak))
        N = G - B

        A, mu, v = fit

        g = multi_gaussian(x_region, fit)

        dS_dA = np.sum(g / A)

        dS_dmu = np.sum(
            g * (x_region - mu) / v
        )

        dS_dv = np.sum(
            g * (x_region - mu)**2 / (2 * v**2)
        )

        grad_area = np.array([
            dS_dA,
            dS_dmu,
            dS_dv,
        ])
        
        peak_area = np.sum(g)
        
        peak_area_var = (
            grad_area
            @ param_cov
            @ grad_area
        )

        peak_area_std = np.sqrt(
            max(peak_area_var, 0.0)
        )
        
        print(peak_area, "+-", peak_area_std)
        print(N)
        

        
        fit_data = Fit(
            region_min,
            region_max,
            lower,
            upper,
            fit,
            err,
            bkg_fit,
            bkg_est_channels,
            G,
            B,
            N,
            peak_area,
        )
        results.append(fit_data)

    return results, converged

if __name__ == "__main__":
    from time import time

    import matplotlib.pyplot as plt
    import pandas as pd
    x_axis, y_axis, _ = pd.read_csv("/home/eewa/Desktop/Cyklotron_Cs.csv").to_numpy().T   
    
    print()
    print("========== DATA ==========")
    print(f"Number of channels: {len(x_axis)}")
    print(f"x range: {x_axis.min()} -> {x_axis.max()}")
    print(f"y range: {y_axis.min()} -> {y_axis.max()}")

    # ------------------------------------------------------------
    # Example bounds
    #
    # CHANGE THESE TO YOUR PEAK WINDOWS
    # ------------------------------------------------------------

    bounds = (
        (550, 750),
    )
    
    
    # ------------------------------------------------------------
    # Run fit
    # ------------------------------------------------------------
    start = time()
    results, converged = fit_gaussians(
        x_axis=x_axis,
        y_axis=y_axis,
        bounds=bounds,
        fit_type="Gaussian",
        use_poisson_weights=False,
        weigh_cov_chi2=True,
        bkg_type="Linear",
        bkg_est_channels=5,
    )
    end = time()
    print(f"Timing: {(end - start) *1e3} ms")

    print()
    print("========== RESULT ==========")
    print(f"Converged: {converged}")

    if results is None:
        print("Fit failed.")

    else:
        for i, result in enumerate(results):


            print(f"Peak {i + 1}")
            print("----------------------------")
            print(result.params)
            print(result.param_errs)

    # ------------------------------------------------------------
    # Plot data, background, and Gaussian fit
    # ------------------------------------------------------------

    plt.figure(figsize=(10, 6))

    # Raw data
    plt.plot(
        x_axis,
        y_axis,
        ".",
        markersize=3,
        label="Data",
        alpha=0.7,
    )

    # Plot only around the ROI
    x_plot = np.linspace(
        bounds[0][0],
        bounds[0][1],
        1000,
    )

    for i, result in enumerate(results):

        # Background
        if result.bkg_params is not None:
            background = np.polyval(
                result.bkg_params,
                x_plot,
            )

            plt.plot(
                x_plot,
                background,
                "--",
                linewidth=2,
                label="Background" if i == 0 else None,
            )

        # Gaussian
        gaussian = multi_gaussian(
            x_plot,
            result.params,
        )

        plt.plot(
            x_plot,
            gaussian + background,
            "-",
            linewidth=2,
            label=f"Gaussian {i + 1}",
        )

    # ROI bounds
    for b in bounds:
        lower, upper = np.min(b), np.max(b)

        plt.axvline(
            lower,
            color="gray",
            linestyle=":",
            alpha=0.7,
        )

        plt.axvline(
            upper,
            color="gray",
            linestyle=":",
            alpha=0.7,
        )

    plt.xlabel("x")
    plt.ylabel("Counts")
    plt.legend()
    plt.grid(alpha=0.2)
    plt.tight_layout()
    plt.show()