#!/bin/sh
# Launch a game with the locally built lsfg-vk layer.
# Usage: ./launch.sh <game-executable> [args...]
#   or as Steam launch option: /home/devind/lsfg-vk/launch.sh %command%
#
# This script enables the lsfg-vk Vulkan implicit layer and
# forwards all arguments to the target executable.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
LAYER_LIB="${SCRIPT_DIR}/build/lsfg-vk-layer/liblsfg-vk-layer.so"

exec env \
    ENABLE_LSFGVK=1 \
    VK_ADD_INSTANCE_LAYERS="VK_LAYER_LSFGVK_frame_generation" \
    VK_ADD_DEVICE_LAYERS="VK_LAYER_LSFGVK_frame_generation" \
    VK_LAYER_PATH="${SCRIPT_DIR}/build/lsfg-vk-layer" \
    "$@"
