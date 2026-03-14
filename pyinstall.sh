#!/bin/bash
# Build MySpect with PyInstaller -- minimal PySide6 + SciPy.optimize + Bleak + qasync + PyUSB

# Activate your conda environment first
# conda activate spect_env

PY_SIDE6_PLUGINS="/home/eewa/anaconda3/envs/spect_env/lib/python3.13/site-packages/PySide6/Qt/plugins"
PY_SIDE6_PLATFORMS="/home/eewa/anaconda3/envs/spect_env/lib/qt6/plugins/platforms"

pyinstaller \
    --name MySpect \
    --onedir \
    --noconfirm \
    --exclude-module PySide6.QtNetwork \
    --exclude-module PySide6.Qt3DCore \
    --exclude-module PySide6.Qt3DRender \
    --exclude-module PySide6.Qt3DExtras \
    --exclude-module PySide6.QtMultimedia \
    --exclude-module PySide6.QtWebEngineWidgets \
    --exclude-module scipy.ndimage \
    --exclude-module scipy.signal \
    --exclude-module matplotlib \
    --exclude-module numba \
    --exclude-module setuptools \
    --exclude-module Cython \
    --exclude-module llvmlite \
    --exclude-module pandas \
    --hidden-import scipy.optimize \
    --hidden-import bleak \
    --hidden-import qasync \
    --hidden-import usb \
    --add-data "${PY_SIDE6_PLATFORMS}:PySide6/Qt/plugins/platforms" \
    GUI_main.py

    #--add-data "${PY_SIDE6_PLUGINS}/platforms:PySide6/Qt/plugins/platforms" \
    #--add-data "${PY_SIDE6_PLUGINS}/imageformats:PySide6/Qt/plugins/imageformats" \