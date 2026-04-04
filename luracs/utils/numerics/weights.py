import numpy as np

def huber_weights(r, yfit, params, delta=1.0):
    abs_r = np.abs(r)
    return np.where(abs_r < delta, 1.0, delta / abs_r)

def poisson_weights(residuals, yfit, params, eps=1e-10):
    return 1.0 / np.maximum(yfit, eps)