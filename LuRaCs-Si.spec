# -*- mode: python ; coding: utf-8 -*-

import os
import sys
import platform
from pathlib import Path

# Platform detection
is_windows = sys.platform.startswith("win")
is_linux = sys.platform.startswith("linux")
is_macos = sys.platform == "darwin"

if is_windows:
    exe_name = "Win"  
elif is_macos:
    exe_name = "MacOS" 
else:
    exe_name = "Linux"
    

ROOT = Path(os.getcwd()).resolve()
__version__ = "0.6.0"

# Cross-platform path
main_script = os.path.join('luracs', 'main.py')
resources = os.path.join('luracs', 'resources')
licences = ROOT / 'licences'

arch = platform.machine().lower()
if arch in ("x86_64", "amd64", "i386", "i686"):
    system_platform = "x86"
elif arch in ("aarch64", "arm64", "armv7l", "armv6l"):
    system_platform = "ARM"

build_config_path = ROOT / "luracs" / "build_config.py"

build_config = f"""
IS_H3=True
IS_Si=True
"""

build_config_path.parent.mkdir(parents=True, exist_ok=True)
build_config_path.write_text(build_config)


exe_name = system_platform + exe_name

bins = []
if is_windows:
    plotext_dll = ROOT / "venv" / "Lib" / "site-packages" / "plotext" / "_kernel" / "cpp" / "kernel.dll"
    bins.append((str(plotext_dll), 'plotext/_kernel/cpp'))

try:
    a = Analysis(
        [str(ROOT / "luracs" / "main.py")],
        pathex=[str(ROOT)],
        binaries=bins,
        datas=[
            (resources, 'resources'),
            (str(licences), 'licences')
            ],
        hiddenimports=[
            'bleak',
            'usb',
            'numpy',
            'requests',
            'plotext',
        ],
        hookspath=[],
        hooksconfig={},
        runtime_hooks=[],
        excludes=[
            'PySide6.Qt3DCore',
            'PySide6.Qt3DRender',
            'PySide6.Qt3DExtras',
            'PySide6.QtMultimedia',

            # Not used - Qt Quick / QML
            'PySide6.QtQml',
            'PySide6.QtQml.Models',
            'PySide6.QtQuick',
            'PySide6.QtQuickWidgets',
            'PySide6.QtQuickControls2',
            'PySide6.QtQuick3D',

            # Webengine exclude
            'PySide6.QtWebEngineCore',
            'PySide6.QtWebEngineWidgets',
            'PySide6.QtWebEngine',

            # Other things than qt
            'cryptography',
            'matplotlib',
            'pillow',
            'numba',
            'setuptools',
            'Cython',
            'llvmlite',
            'pandas',
            'PyQt6',
            'PyQt5',
            'qasync',
            'scipy',
            'PIL',
        ],
        
        noarchive=False,
        optimize=0,
    )

    # Remove unnecessary PySide6 Qt data
    a.datas = [
        item for item in a.datas
        if not (
            item[0].startswith('PySide6/Qt/qml/')
            or item[0].startswith('PySide6/Qt/translations/')
            or item[0].endswith('qtwebengine_devtools_resources.pak')
        )
    ]


    pyz = PYZ(a.pure)
    
    if is_windows:
        exe_suffix = ".exe"  
    elif is_macos:
        exe_suffix = ".app"
    else:
        exe_suffix = ""

    exe = EXE(
        pyz,
        a.scripts,
        [],
        exclude_binaries=True,
        name='LuRaCs-Si'+ exe_suffix,
        debug=False,
        bootloader_ignore_signals=False,
        strip=False,   # strip only on Linux (safe)
        upx=is_windows,   # UPX works better on Windows
        console=not is_windows,  # GUI app on Windows, console on Linux
        disable_windowed_traceback=False,
        argv_emulation=False,
        target_arch=None,
        codesign_identity=None,
        entitlements_file=None,
        icon=str(ROOT / "dev/main_icon_green.ico"),
    )

    coll = COLLECT(
        exe,
        a.binaries,
        a.datas,
        strip=False,
        upx=is_windows,
        upx_exclude=[],
        name=f'LuRaCs-Si_{__version__}_{exe_name}',
    )

finally:    
    build_config_path.unlink(missing_ok=True)