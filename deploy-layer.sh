#!/bin/bash
# Deploy the custom lsfg-vk layer to ALL locations.
# Run after building: cmake --build build && ./deploy-layer.sh
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
LAYER_SO="$SCRIPT_DIR/build/lsfg-vk-layer/liblsfg-vk-layer.so"

if [ ! -f "$LAYER_SO" ]; then
    echo "ERROR: $LAYER_SO not found. Run cmake --build first."
    exit 1
fi

REF_MD5=$(md5sum "$LAYER_SO" | cut -d' ' -f1)
echo "Deploying: $LAYER_SO  (md5: $REF_MD5)"
echo ""

# 1. System libraries (these are what VK_LAYER_LS_frame_generation loads)
echo "=== System libraries ==="
for f in /usr/lib/liblsfg-vk.so /usr/lib/liblsfg-vk-layer.so /usr/local/lib/liblsfg-vk-layer.so; do
    sudo cp "$LAYER_SO" "$f"
    echo "  $f  OK"
done
echo ""

# 2. Proton Experimental
echo "=== Proton ==="
STEAM_COMMON="$HOME/.local/share/Steam/steamapps/common"
for PROTON_DIR in \
    "$STEAM_COMMON/Proton - Experimental" \
    "$STEAM_COMMON/Proton - Experimental (Beta)"; do
    if [ -d "$PROTON_DIR/files/lib/x86_64-linux-gnu" ]; then
        cp "$LAYER_SO" "$PROTON_DIR/files/lib/x86_64-linux-gnu/liblsfg-vk.so"
        echo "  $(basename "$PROTON_DIR")/files/lib/.../liblsfg-vk.so  OK"
    fi
done

# GE-Proton
if [ -d "$HOME/.local/share/Steam/compatibilitytools.d" ]; then
    for d in "$HOME/.local/share/Steam/compatibilitytools.d"/GE-Proton*; do
        if [ -d "$d/files/lib/x86_64-linux-gnu" ]; then
            cp "$LAYER_SO" "$d/files/lib/x86_64-linux-gnu/liblsfg-vk.so"
            echo "  $(basename "$d")/files/lib/.../liblsfg-vk.so  OK"
        fi
    done
fi
echo ""

# 3. Steam Runtime pressure-vessel overrides
echo "=== Steam Runtimes ==="
find "$STEAM_COMMON" "$HOME/.local/share/Steam/steamrt64" \
    -name "liblsfg-vk*.so" -not -path "*/lsfg-vk/*" 2>/dev/null | while read -r f; do
    cp "$LAYER_SO" "$f"
    echo "  $f  OK"
done
echo ""

# 4. Verify all copies match
echo "=== Verification ==="
MISMATCH=0
find /usr/local/lib /usr/lib "$STEAM_COMMON" "$HOME/.local/share/Steam/steamrt64" \
    -name "liblsfg-vk*.so" -not -path "*/lsfg-vk/*" 2>/dev/null | while read -r f; do
    m=$(md5sum "$f" | cut -d' ' -f1)
    if [ "$m" = "$REF_MD5" ]; then
        echo "  OK  $f"
    else
        echo "  MISMATCH  $f"
    fi
done

echo ""
echo "Done. Restart any running games to load the new layer."
