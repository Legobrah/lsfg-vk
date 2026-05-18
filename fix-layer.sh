#!/bin/bash
# Fix the Vulkan layer manifest for lsfg-vk v2 and set up Proton compatibility.
#
# The pacman-installed JSON may have v1 format with function pointers that don't
# exist in the v2 binary. The v2 binary uses vkNegotiateLoaderLayerInterfaceVersion.
#
# This script also:
#   - Copies the .so into Proton's lib directory (for Steam Proton games)
#   - Creates a user-space layer JSON with absolute library_path
#
# Run with: sudo bash fix-layer.sh

set -e

echo "=== Step 1: Fix Vulkan layer manifest ==="
echo "Writing /etc/vulkan/implicit_layer.d/VkLayer_LS_frame_generation.json ..."

cat > /etc/vulkan/implicit_layer.d/VkLayer_LS_frame_generation.json << 'EOF'
{
  "file_format_version": "1.1.0",
  "layer": {
    "name": "VK_LAYER_LS_frame_generation",
    "description": "Lossless Scaling frame generation layer",
    "implementation_version": "2",
    "library_path": "liblsfg-vk.so",
    "type": "GLOBAL",
    "api_version": "1.4.328",
    "disable_environment": {
      "DISABLE_LSFG": "1"
    }
  }
}
EOF

echo "  - Removed stale 'functions' block (v2 uses negotiation interface)"
echo "  - Set file_format_version to 1.1.0"
echo "  - Set api_version to 1.4.328"
echo ""

echo "=== Step 2: Set up Proton compatibility ==="

# Find Proton installations
PROTON_DIRS=(
  "$HOME/.local/share/Steam/steamapps/common/Proton - Experimental"
  "$HOME/.local/share/Steam/steamapps/common/Proton - Experimental (Beta)"
)

# Also check GE-Proton versions in compatibilitytools.d
if [ -d "$HOME/.local/share/Steam/compatibilitytools.d" ]; then
  for d in "$HOME/.local/share/Steam/compatibilitytools.d"/GE-Proton*; do
    [ -d "$d" ] && PROTON_DIRS+=("$d")
  done
fi

LAYER_SO="/usr/lib/liblsfg-vk.so"
if [ ! -f "$LAYER_SO" ]; then
  echo "WARNING: $LAYER_SO not found. Skipping Proton setup."
  echo "Install lsfg-vk first, then re-run this script."
  exit 0
fi

for PROTON_DIR in "${PROTON_DIRS[@]}"; do
  if [ ! -d "$PROTON_DIR" ]; then
    continue
  fi

  PROTON_LIB="$PROTON_DIR/files/lib/x86_64-linux-gnu"
  if [ ! -d "$PROTON_LIB" ]; then
    echo "  Skipping $PROTON_DIR (no lib dir)"
    continue
  fi

  PROTON_NAME=$(basename "$PROTON_DIR")

  # Copy the .so (not symlink -- container can't follow host symlinks)
  cp "$LAYER_SO" "$PROTON_LIB/liblsfg-vk.so"
  echo "  Copied liblsfg-vk.so -> $PROTON_NAME/files/lib/x86_64-linux-gnu/"

  # Create user-space layer JSON with absolute path
  USER_LAYER_DIR="$HOME/.local/share/vulkan/implicit_layer.d"
  mkdir -p "$USER_LAYER_DIR"

  cat > "$USER_LAYER_DIR/VkLayer_LS_frame_generation.json" << EOFJSON
{
  "file_format_version": "1.1.0",
  "layer": {
    "name": "VK_LAYER_LS_frame_generation",
    "description": "Lossless Scaling frame generation layer",
    "implementation_version": "2",
    "library_path": "$PROTON_LIB/liblsfg-vk.so",
    "type": "GLOBAL",
    "api_version": "1.4.328",
    "disable_environment": {
      "DISABLE_LSFG": "1"
    }
  }
}
EOFJSON
  echo "  Created $USER_LAYER_DIR/VkLayer_LS_frame_generation.json"
  echo ""

done

echo "=== Done ==="
echo ""
echo "Steam launch option for Proton games:"
echo "  VK_INSTANCE_LAYERS=VK_LAYER_LS_frame_generation %command%"
echo ""
echo "Native Linux games: no launch option needed (implicit layer loads automatically)."
echo ""
echo "Verifying layer is visible to vulkan loader..."
VK_INSTANCE_LAYERS=VK_LAYER_LS_frame_generation vulkaninfo --summary 2>&1 | grep -i 'LS_frame\|error' | head -5
