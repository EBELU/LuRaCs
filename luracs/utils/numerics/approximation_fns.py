from collections.abc import Callable
from enum import Enum

import numpy as np

"""Functions for approximating physical relationships. In order to save them efficiently in string format for XML are they wrapped in container classes with StrEnums has accessors. This makes is easy to find the function from the string as *signature=EfficiencyFn.Signatures(string)*, which can now be used to get the correct function. Function shall be written with '{}' representing parameters to optimized."""


def exp_atten(E: float, params: np.ndarray) -> float:
    if len(params) != 2:
        raise ValueError(
            f"Number of parameters must be 2! Number of given parameters {len(params)}"
        )
    C0, C1 = params
    return C0 * np.exp(-C1 * E)


def exp_polynomial(E: float, params: np.ndarray) -> float:
    if len(params) != 4:
        raise ValueError(
            f"Number of parameters must be 4! Number of given parameters {len(params)}"
        )
    A, B, C, D = params
    return np.exp(A + B * np.log(E) + C * np.log(E) ** 2 + D * np.log(E) ** 3)



class EfficiencyFn:
    "Container class for functions approximating the energy efficiency of detectors"
    class Signatures(Enum):
        EXP_ATTEN = "{}*EXP(-{}*E)"
        EXP_POLY = "EXP({}+{}*LOG(E)+{}*LOG(E)^2+{}*LOG(E)^3)"
        
        
    @classmethod
    def lookup(cls, signature: Signatures) -> Callable:
        "Look up a function based on its signature"
        lookup_table = {
            cls.Signatures.EXP_ATTEN: exp_atten,
            cls.Signatures.EXP_POLY: exp_polynomial
        }
        return lookup_table[signature]
    
def resolution(E, a):
    return a / np.sqrt(E)
        
class ResolutionFn:
    "Container class for functions approximating the energy resolution of detectors"
    class Signatures(Enum):
        K_OVER_SQRT_E = "{}/SQRT(E)"
        
    @classmethod
    def lookup(cls, signature: Signatures) -> Callable:
        "Look up a function based on its signature"
        lookup_table = {
            cls.Signatures.K_OVER_SQRT_E: resolution
        }
        return lookup_table[signature]




