#!/bin/bash
set -e
echo "Checking dependency budget..."
DEP_COUNT=$(cargo tree --workspace | wc -l)
if [ "$DEP_COUNT" -gt 300 ]; then
  echo "::warning::Dependency count ($DEP_COUNT) exceeds budget of 300!"
fi
echo "Checking file budget..."
FILE_COUNT=$(find native/src core/src -name "*.rs" | wc -l)
if [ "$FILE_COUNT" -gt 150 ]; then
  echo "::warning::File count ($FILE_COUNT) exceeds budget of 150!"
fi
echo "Budget checks complete."
