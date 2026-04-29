from uncertainties import ufloat, Variable
import uncertainties.umath as umath
import numpy as np

def u_intrinsic_efficiency(
                        activity_Bq: Variable, 
                        source_detector_distance_cm: Variable,
                        emission_yield: Variable,
                        measurement_time_s: Variable,
                        detector_area_cm2: Variable,
                        measured_counts: Variable
                        ) -> Variable:
    
    cps = measured_counts / measurement_time_s
    solid_angle = detector_area_cm2 / (source_detector_distance_cm**2 * 4 * np.pi)
    return cps / (activity_Bq * emission_yield * solid_angle)



