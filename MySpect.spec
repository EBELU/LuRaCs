# -*- mode: python ; coding: utf-8 -*-


a = Analysis(
    ['GUI_main.py'],
    pathex=[],
    binaries=[],
    datas=[('/home/eewa/anaconda3/envs/spect_env/lib/qt6/plugins/platforms', 'PySide6/Qt/plugins/platforms')],
    hiddenimports=['scipy.optimize', 'bleak', 'qasync', 'usb'],
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=['PySide6.QtNetwork', 'PySide6.Qt3DCore', 'PySide6.Qt3DRender', 'PySide6.Qt3DExtras', 'PySide6.QtMultimedia', 'PySide6.QtWebEngineWidgets', 'scipy.ndimage', 'scipy.signal', 'matplotlib', 'numba', 'setuptools', 'Cython', 'llvmlite', 'pandas'],
    noarchive=False,
    optimize=0,
)
pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name='MySpect',
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=True,
    console=True,
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
    strip=False,
    upx=True,
    upx_exclude=[],
    name='MySpect',
)
