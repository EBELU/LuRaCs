$ErrorActionPreference = "Stop"

# pytest && python -m build && python -m twine check dist/*

# Build all PyInstaller spec files
Get-ChildItem -Path "." -Filter "*.spec" -File | ForEach-Object {
    pyinstaller -y $_.FullName
    if ($LASTEXITCODE -ne 0) {
        throw "PyInstaller failed for $($_.Name)"
    }
}

# Package a directory into a 7z archive and generate its SHA-256 checksum
function Package7z {
    param(
        [string]$Name,
        [string]$Dir
    )

    $archive = "dist\$Name.7z"
    $checksum = "dist\$Name-sha256sum"

    # Create a 7z archive with maximum compression
    & "C:\Program Files\7-Zip\7z.exe" a -tzip -mx=9 $archive $Dir
    if ($LASTEXITCODE -ne 0) {
        throw "7-Zip failed for $Name"
    }

    # Generate SHA-256 checksum
    $hash = (Get-FileHash -Path $archive -Algorithm SHA256).Hash.ToLower()
    "$hash  $Name.7z" | Set-Content -Path $checksum -Encoding ascii
}

# Package a directory into a standard ZIP archive and generate its SHA-256 checksum
function Package {
    param(
        [string]$Name,
        [string]$Dir
    )

    $archive = "dist\$Name.zip"
    $checksum = "dist\$Name-sha256sum"

    # Create a standard ZIP archive with maximum compression
    & "C:\Program Files\7-Zip\7z.exe" a -tzip -mx=9 $archive $Dir

    if ($LASTEXITCODE -ne 0) {
        throw "7-Zip failed for $Name"
    }

    # Generate SHA-256 checksum
    $hash = (Get-FileHash -Path $archive -Algorithm SHA256).Hash.ToLower()
    "$hash  $Name.zip" | Set-Content -Path $checksum -Encoding ascii
}

# Package each directory under dist
Get-ChildItem -Path "dist" -Directory | ForEach-Object {
    Package -Name $_.Name -Dir $_.FullName
}