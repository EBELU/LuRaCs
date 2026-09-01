# cython: language_level=3
# cython: boundscheck=False
# cython: wraparound=False
# cython: cdivision=True

import numpy as np
cimport numpy as cnp

from libc.math cimport exp, log, fmax, sqrt, NAN

ctypedef cnp.float64_t float64_t
ctypedef cnp.int32_t int32_t
ctypedef cnp.int64_t int64_t


cdef Py_ssize_t find_index_from_energy(
    double E, 
    double[:] array
    ) noexcept nogil:
    "Returns the index of the first value satisfying array > E, so array[i-1] < E < array[i]"
    cdef Py_ssize_t i
    i = 0
    while i < array.shape[0] and array[i] < E:
        i += 1
        
    if i == array.shape[0]:
        return -1
    else:
        return i
     
cdef double linear_interpolation(
    double x, 
    double x1, 
    double x2, 
    double y1, 
    double y2
    ) noexcept nogil:
    "Basic linear interpolation"
    return y1 + (x - x1) * (y2 - y1) / (x2 - x1)

cdef double interpolate_point_on_axis(
    double x,
    double[:] x_array,
    double[:] y_array
) noexcept nogil:
    cdef Py_ssize_t i

    i = find_index_from_energy(x, x_array)

    if i <= 0 or i >= x_array.shape[0]:
        return NAN

    return linear_interpolation(
        x,
        x_array[i - 1],
        x_array[i],
        y_array[i - 1],
        y_array[i]
    )

    

cdef double get_backscatter_E(
    double E_p
    ) noexcept nogil:
    "Calculate the backscatter energy from the photon energy"
    return E_p / (1. + 2. * E_p / 511.0)

cpdef list calculate_regions(
    double E_p, 
    double max_value, 
    bool has_escape_peaks
    ):
    cdef:
        double backscatter, compton, bellow_peak, above_peak, single_escape, double_escape

    backscatter = get_backscatter_E(E_p)
    compton = E_p - backscatter
    bellow_peak = E_p - 10
    above_peak = E_p + 10
    
    if not has_escape_peaks:
        return [0, backscatter, compton, bellow_peak, above_peak, max_value]
    
    else:
        single_escape = E_p - 511.
        double_escape = E_p - 1022.
        return sorted([0, backscatter, compton, bellow_peak, above_peak, single_escape, double_escape, max_value])

cdef double interpolate_point(
    double j_k, 
    double E_i, 
    double E_1, 
    double E_2, 
    double[:] E_1_regions, 
    double[:] E_2_regions, 
    double[:] E_i_regions, 
    double[:] ref_E_axis, 
    double[:] requested_E_axis, 
    double[:] y_1, 
    double[:] y_2
    ) noexcept nogil:
    cdef:
        Py_ssize_t k
        double ei_jk, e1_jk, e2_jk
        double yi_jk, y1_jk, y2_jk

    # Determine region point
    k = find_index_from_energy(j_k, E_i_regions)
    
    # Like the paper
    ei_jk = j_k
    
    # Calculate the corresponding points on the E_1 and E_2 rows
    e1_jk = (ei_jk - E_i_regions[k - 1]) / (E_i_regions[k] - E_i_regions[k - 1]) * (E_1_regions[k] - E_1_regions[k - 1]) + E_1_regions[k - 1]
    e2_jk = (ei_jk - E_i_regions[k - 1]) / (E_i_regions[k] - E_i_regions[k - 1]) * (E_2_regions[k] - E_2_regions[k - 1]) + E_2_regions[k - 1]
    
    # Get the y-values
    y1_jk = interpolate_point_on_axis(e1_jk, ref_E_axis, y_1)
    y2_jk = interpolate_point_on_axis(e2_jk, ref_E_axis, y_2)
    
    # Interpolate the 
    yi_jk = linear_interpolation(E_i, E_2, E_1, y2_jk, y1_jk)
    return yi_jk
    
    

cdef void interpolate_response_row(
    double E_p, 
    double[:] requested_E_axis, 
    double[:] ref_E_axis, 
    double low_E, 
    double[:] low_row, 
    double high_E, 
    double[:] high_row, 
    double[:] row_buffer
    ):
    cdef:
        cnp.ndarray[cnp.float64_t, ndim=1] low_regions, high_regions, E_i_regions
        double[:] low_regions_v, high_regions_v, E_i_regions_v
        bool has_escape_peaks

    if low_E > 1022.:
        has_escape_peaks = True
    else:
        has_escape_peaks = False
        
    # Determine low and high regions in the reference matrix, then interpolate regions to the new row
    low_regions = np.asarray(calculate_regions(low_E, np.max(requested_E_axis), has_escape_peaks), dtype=np.float64)
    high_regions = np.asarray(calculate_regions(high_E, np.max(requested_E_axis), has_escape_peaks), dtype=np.float64)

    E_i_regions = (E_p - low_E) / (high_E - low_E) * (high_regions - low_regions) + low_regions

    low_regions_v = low_regions
    high_regions_v = high_regions
    E_i_regions_v = E_i_regions

    with nogil:     
        for i in range(requested_E_axis.shape[0]):
            row_buffer[i] = (interpolate_point(requested_E_axis[i], E_p, high_E, low_E, high_regions_v, low_regions_v, E_i_regions_v, ref_E_axis, requested_E_axis, high_row, low_row))

cpdef tuple process_matrix_cy(
        cnp.ndarray[cnp.float64_t, ndim=2] response_matrix,
        cnp.ndarray[cnp.float64_t, ndim=1] ref_indices,
        cnp.ndarray[cnp.float64_t, ndim=1] ref_E_axis,
        cnp.ndarray[cnp.float64_t, ndim=1] requested_indices,
        cnp.ndarray[cnp.float64_t, ndim=1] requested_E_axis
        ):
    
    cdef:
        cnp.ndarray[cnp.float64_t, ndim=2] new_matrix
        cnp.ndarray[cnp.float64_t, ndim=1] high_row, low_row, row_buffer
        Py_ssize_t high_row_i
        double high_E, low_E
        

        # Input memoryviews
        const double[:, :] response_matrix_v
        const double[:] ref_indices_v
        const double[:] ref_E_axis_v
        const double[:] requested_indices_v
        const double[:] requested_E_axis_v

        # Output / working memoryviews
        double[:, :] new_matrix_v
        double[:] high_row_v
        double[:] low_row_v
        double[:] row_buffer_v

        list offsets = [0]
        list indices = []
        list values = []


    # new_matrix = np.empty((len(requested_indices), len(requested_E_axis)))
    row_buffer = np.empty(requested_E_axis.shape[0], dtype=np.float64)

    response_matrix_v = response_matrix
    ref_indices_v = ref_indices
    ref_E_axis_v = ref_E_axis
    requested_indices_v = requested_indices
    requested_E_axis_v = requested_E_axis

    # new_matrix_v = new_matrix
    row_buffer_v = row_buffer

    
    for i in range(requested_indices.shape[0]):        
        high_row_i = find_index_from_energy(requested_indices[i], ref_indices)
        high_E = ref_indices[high_row_i]
        low_E = ref_indices[high_row_i - 1]
        high_row_v = response_matrix[high_row_i] / np.sum(response_matrix[high_row_i])
        low_row_v = response_matrix[high_row_i - 1] / np.sum(response_matrix[high_row_i - 1])
        
        interpolate_response_row(requested_indices[i], requested_E_axis, ref_E_axis, low_E, low_row_v, high_E, high_row_v, row_buffer)
        row_buffer /= np.nansum(row_buffer)

        for j in range(row_buffer.shape[0]):
            if row_buffer[j] > 1e-12:
                indices.append(j)
                values.append(row_buffer[j])

        offsets.append(len(values))
        
    return (
        np.asarray(offsets, dtype=np.int64),
        np.asarray(indices, dtype=np.int32),
        np.asarray(values, dtype=np.float64),
    )