import numpy as np

def exp_atten(E: float, params: np.ndarray) -> float:
    if len(params) != 2:
        raise ValueError(f"Number of parameters must be 2! Number of given parameters {len(params)}")
    C0, C1 = params
    return C0 * np.exp(-C1 * E)

def exp_polynomial(E: float, params: np.ndarray) -> float:
    if len(params) != 4:
        raise ValueError(f"Number of parameters must be 4! Number of given parameters {len(params)}")
    A, B, C, D = params
    return np.exp(A + B * np.log(E) + C * np.log(E)**2 + D * np.log(E)**3)

def resolution(E, a):
    return a / np.sqrt(E)