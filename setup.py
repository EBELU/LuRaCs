from Cython.Build import cythonize
from setuptools import Extension, setup

import numpy as np


cython_files = [
    "gaussian",
    "deconvolution",
    "rebin",
]

extensions = [
    Extension(
        name=f"luracs.utils.numerics.cython.{cy_f}",
        sources=[f"luracs/utils/numerics/cython/{cy_f}.pyx"],
        include_dirs=[
            np.get_include(),
        ],
        extra_compile_args=[
        "-O3",
        ],
    ) for cy_f in cython_files
]

setup(
    name="luracs",
    ext_modules=cythonize(
        extensions,
        compiler_directives={"language_level": "3"},
    ),
    zip_safe=False,
)