# -*- mode: python ; coding: utf-8 -*-

import os
import sys

# Platform detection
is_windows = sys.platform.startswith("win")
is_linux = sys.platform.startswith("linux")
exe_name = "Win" if is_windows else "Linux"

# Cross-platform path
main_script = os.path.join('luracs', 'main.py')

a = Analysis(
    [main_script],
    pathex=[],
    binaries=[],
    datas=[],
    hiddenimports=[
        'bleak',
        'qasync',
        'usb'
    ],
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[
        'PySide6.QtNetwork',
        'PySide6.Qt3DCore',
        'PySide6.Qt3DRender',
        'PySide6.Qt3DExtras',
        'PySide6.QtMultimedia',
        'matplotlib',
        'numba',
        'setuptools',
        'Cython',
        'llvmlite',
        'pandas',
        'PyQt6',
        'PyQt5',
    ],
    noarchive=False,
    optimize=0,
)

pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name='LuRaCs-H3'+ exe_name,
    debug=False,
    bootloader_ignore_signals=False,
    strip=is_linux,   # strip only on Linux (safe)
    upx=is_windows,   # UPX works better on Windows
    console=not is_windows,  # GUI app on Windows, console on Linux
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
)

coll = COLLECT(
    exe,
    a.binaries,
    a.datas,
    strip=is_linux,
    upx=is_windows,
    upx_exclude=[],
    name='LuRaCs-H3',
)
