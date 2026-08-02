from setuptools import setup, Extension
from Cython.Build import cythonize
import numpy as np
import sys


extra_compile_args = []
extra_link_args = []

# Enable OpenMP for prange
if sys.platform == "win32":
    extra_compile_args += ["/openmp"]
else:
    extra_compile_args += ["-O3", "-fopenmp"]
    extra_link_args += ["-fopenmp"]

extensions = [
    Extension(
        name="richardson_lucy",
        sources=["dev/debug/richardson_lucy.pyx"],
        include_dirs=[
            np.get_include(),
        ],
        # extra_compile_args=extra_compile_args,
        # extra_link_args=extra_link_args,
    ),
    Extension(
    name="interpolator",
    sources=["dev/debug/interpolator.pyx"],
    include_dirs=[np.get_include()],
    extra_compile_args=extra_compile_args,
    extra_link_args=extra_link_args,
    ),
]


setup(
    name="richardson_lucy",
    version="0.1.0",
    description="Cython accelerated Richardson-Lucy reconstruction",
    ext_modules=cythonize(
        extensions,
        compiler_directives={
            "language_level": 3,
            "boundscheck": False,
            "wraparound": False,
            "initializedcheck": True,
            "nonecheck": True,
            "cdivision": True,
        },
    ),
    install_requires=[
        "numpy",
        "cython",
    ],
)