#!/bin/bash

python -m nuitka \
    --standalone \
    --include-data-dir=luracs/resources=luracs/resources \
    --enable-plugin=pyside6 \
    luracs/main.py