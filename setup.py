
import numpy as np
from Cython.Build import cythonize
from setuptools import Extension, setup

cython_files = [
    "gaussian",
    "deconvolution",
    "rebin",
    "mariscotti",
    "process_matrix"
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