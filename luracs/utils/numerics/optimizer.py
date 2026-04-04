import numpy as np

def numerical_jacobian(f, x, eps=1e-8):
    """
    Compute the Jacobian of f at x numerically using finite differences.
    
    Parameters
    ----------
    f : function
        f(x) should return a 1D array of length m
    x : array_like
        Current parameter vector (length n)
    eps : float
        Step size for finite differences
    
    Returns
    -------
    J : ndarray, shape (m, n)
        Jacobian matrix
    """
    f0 = f(x)
    m = len(f0)
    n = len(x)
    J = np.zeros((m, n))
    
    for j in range(n):
        dx = np.zeros_like(x)
        dx[j] = eps
        f1 = f(x + dx)
        J[:, j] = (f1 - f0) / eps
    return J

def curve_fit(
    f,
    xdata,
    ydata,
    p0,
    jac=None,
    weight_fn=None,
    weight_cov_chi2=True,
    max_iter=100,
    tol=1e-8,
    lam=1e-3
):
    """
    Levenberg-Marquardt optimizer with flexible weighting.

    Parameters
    ----------
    f : callable
        Model function f(xdata, params) -> y
    xdata : array_like
        Independent variable
    ydata : array_like
        Observed data
    p0 : array_like
        Initial parameter guess
    jac : callable, optional
        Jacobian J(xdata, params) -> shape (N, M)
    weight_fn : callable, optional
        weight_fn(residuals, yfit, params) -> weights (N,)
    """

    p = np.array(p0, dtype=float)
    xdata = np.array(xdata, dtype=float)
    ydata = np.array(ydata, dtype=float)

    converged = False

    for _ in range(max_iter):
        yfit = f(xdata, p)
        r = ydata - yfit

        # Jacobian
        if jac is not None:
            J = jac(xdata, p)
        else:
            J = numerical_jacobian(lambda params: f(xdata, params), p)

        # Apply weights
        if weight_fn is not None:
            w = np.asarray(weight_fn(r, yfit, p))
            W = np.sqrt(w)
            r_w = W * r
            J_w = W[:, None] * J
        else:
            r_w = r
            J_w = J

        # Normal equations
        H = J_w.T @ J_w
        g = J_w.T @ r_w

        # Damped system
        A = H + lam * np.eye(len(p))

        try:
            dp = np.linalg.solve(A, g)
        except np.linalg.LinAlgError:
            dp = np.linalg.lstsq(A, g, rcond=None)[0]

        # Trial step
        p_new = p + dp
        yfit_new = f(xdata, p_new)
        r_new = ydata - yfit_new

        if weight_fn is not None:
            w_new = np.asarray(weight_fn(r_new, yfit_new, p_new))
            r_new_w = np.sqrt(w_new) * r_new
        else:
            r_new_w = r_new

        # Accept/reject
        if np.linalg.norm(r_new_w) < np.linalg.norm(r_w):
            p = p_new
            lam *= 0.5
        else:
            lam *= 2.0

        # Convergence checks
        if np.linalg.norm(dp) < tol * (np.linalg.norm(p) + tol):
            converged = True
            break

        if np.linalg.norm(g, ord=np.inf) < tol:
            converged = True
            break

    # Final Jacobian
    if jac is not None:
        J = jac(xdata, p)
    else:
        J = numerical_jacobian(lambda params: f(xdata, params), p)

    if weight_fn is not None:
        w = np.asarray(weight_fn(ydata - f(xdata, p), f(xdata, p), p))
        J = np.sqrt(w)[:, None] * J

    H = J.T @ J

    # Covariance
    try:
        cov = np.linalg.inv(H)
    except np.linalg.LinAlgError:
        cov = np.linalg.pinv(H)

    # Chi^2 scaling
    if weight_cov_chi2:
        residuals = ydata - f(xdata, p)
        dof = max(len(ydata) - len(p), 1)
        chi2 = np.sum(residuals**2) / dof
        cov *= chi2

    return p, cov, converged