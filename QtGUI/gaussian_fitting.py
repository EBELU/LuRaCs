import numpy as np
from scipy.optimize import curve_fit
from typing import Callable, List, Optional, Tuple

# ----------------- GAUSSIAN FUNCTION -----------------
def GaussFunc(x: np.ndarray, A: float, mu: float, sigma: float) -> np.ndarray:
    return A * np.exp(-0.5 * ((x - mu)/sigma)**2)


# ----------------- GAUSSIAN CLASS -----------------
class Gaussian:
    def __init__(
        self,
        A: float,
        mu: float,
        sigma: float,
        covar_matrix: np.ndarray,
        x: np.ndarray,
        y: np.ndarray,
        region_limits: Optional[List[Optional[float]]] = [None, None],
        corr_f: Callable[[np.ndarray], np.ndarray] = lambda x: 0,
        corr_points: Optional[List[Optional[float]]] = [None, None],
        G: float = 0,
        B: float = 0,
        N: float = 0

    ):
        self.A: float = A
        self.mu: float = mu
        self.sigma: float = sigma
        self.covar_matrix: np.ndarray = covar_matrix
        self._x: np.ndarray = x
        self._y: np.ndarray = y
        self._region_limits: List[Optional[float]] = region_limits
        self._corr_f: Callable[[np.ndarray], np.ndarray] = corr_f
        self._corr_points: List[Optional[float]] = corr_points
        self.G: float = G
        self.N: float = N
        self.B: float = B

    # ---------- GETTERS ----------
    def value(self, x: np.ndarray) -> np.ndarray:
        return GaussFunc(x, self.A, self.mu, self.sigma)

    def area(self) -> float:
        return self.A * abs(self.sigma) * np.sqrt(2 * np.pi)

    def FWHM(self, uncertainty: bool = False) -> float | np.ndarray:
        if uncertainty:
            return 2.35 * np.array([self.sigma, np.sqrt(self.covar_matrix[2][2])])
        else:
            return 2.35 * self.sigma

    def max_height(self) -> float:
        return self.A + self._corr_f(self.mu)

    def __str__(self) -> str:       
        return_str = ("--- Gaussian Parameters ---\n"
                    f"A = {self.A:,.1f} μ = {self.mu:,.1f} σ = {self.sigma:,.2f}\n"
                    "--- Counts ---"
                    f"\nG={self.G:,.0f} N={self.N:,.0f} B={self.B:,.0f}\n"
                    f"Max height={self.max_height():,.0f}"
        ).replace(",", " ")
        return return_str


# ----------------- GAUSSIAN FIT FUNCTION -----------------
def fit_gaussian(
    X: np.ndarray,
    Y: np.ndarray,
    region_start: float,
    region_stop: float,
    corr_left: Optional[float] = None,
    corr_right: Optional[float] = None,
    mu_guess: Optional[float] = None,
    A_guess: Optional[float] = None,
    sigma_guess: Optional[float] = None,
    scatter_corr: bool = True,
    scatter_corr_points: int = 3
) -> Gaussian:

    X, Y = np.array(X), np.array(Y)
    region_mask = (X >= region_start) & (X <= region_stop)
    x_region = X[region_mask]
    y_region = Y[region_mask]
    const_y_region = y_region.copy()
    
    if not corr_left:
        corr_left = region_start
        
    if not corr_right:
        corr_right = region_stop

    # ---------- SCATTER CORRECTION ----------
    if scatter_corr:
        left_x = np.mean(X[X >= corr_left][:scatter_corr_points])
        right_x = np.mean(X[X <= corr_right][-scatter_corr_points:])
        left_y = np.mean(Y[X >= corr_left][:scatter_corr_points])
        right_y = np.mean(Y[X <= corr_right][-scatter_corr_points:])
        k, m = np.polyfit([left_x, right_x], [left_y, right_y], 1)

        def corr_f(x: np.ndarray) -> np.ndarray:
            return k * x + m

        y_region = const_y_region - corr_f(x_region)
        corr_points = [corr_left, corr_right]

        G = float(np.sum(const_y_region))
        B = float(np.sum(corr_f(x_region)))
        N = G - B

        print(G, B, N)
    else:
        def corr_f(x: np.ndarray) -> np.ndarray:
            return np.zeros_like(x)

        corr_points = [None, None]
        G = float(np.sum(y_region))
        B = 0
        N = float(np.sum(y_region))


    # ---------- INITIAL GUESS ----------
    if A_guess is None:
        A_guess = float(max(y_region))
    if mu_guess is None:
        mu_guess = float(x_region[np.argmax(y_region)])
    if sigma_guess is None:
        sigma_guess = float((region_stop - region_start)/6)  # rough guess

    guesses = [A_guess, mu_guess, sigma_guess]

    # ---------- FIT ----------
    try:
        estimates, covar_matrix = curve_fit(GaussFunc, x_region, y_region, p0=guesses)
        return Gaussian(
            estimates[0], estimates[1], abs(estimates[2]),
            covar_matrix, x_region, const_y_region,
            region_limits=[region_start, region_stop],
            corr_f=corr_f, corr_points=corr_points, G=G, B=B, N=N
        )
    except Exception as e:
        raise RuntimeError(f"Gaussian fit failed in region [{region_start}, {region_stop}]: {e}")
