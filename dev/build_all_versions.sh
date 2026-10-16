#!/bin/bash
set -e

pytest && python -m build && python -m twine check dist/*

for spec in *.spec; do
    pyinstaller -y "$spec"
done
wait

function package(){
    name=$1
    dir=$2

    7z a -t7z -mx=9 "dist/${name}.7z" "$dir"
    sha256sum "dist/${name}.7z" > dist/${name}-sha256sum
}

for dir in dist/*/; do
    [ -d "$dir" ] || continue
    name="${dir%/}"
    name="${name##*/}"

    package $name $dir &

done
wait