/* SPDX-License-Identifier: GPL-3.0-or-later */

#pragma once

#include "lsfg-vk-common/vulkan/image.hpp"
#include "lsfg-vk-common/vulkan/buffer.hpp"
#include "lsfg-vk-common/vulkan/vulkan.hpp"

#include <cstdint>
#include <string>
#include <vector>

#include <vulkan/vulkan_core.h>

namespace lsfgvk::layer {

    /// In-game overlay OSD rendered directly onto the swapchain.
    ///
    /// Uses a hardcoded 5x8 bitmap font to render metrics text
    /// (FPS, latency, multiplier, profile name) onto a small RGBA
    /// pixel buffer, uploads it to a Vulkan image, and blits it
    /// onto the top-left corner of each presented frame.
    class Overlay {
    public:
        /// Create the overlay.
        /// @param vk vulkan instance
        /// @param profileName name of the active profile (for display)
        /// @param multiplier frame generation multiplier
        Overlay(const vk::Vulkan& vk,
            const std::string& profileName,
            uint32_t multiplier);

        /// Update the overlay text and re-upload to GPU.
        /// @param vk vulkan instance
        /// @param realFPS real input FPS
        /// @param outputFPS output FPS after FG
        /// @param nativeLatencyMs native latency in ms (without FG)
        /// @param fgLatencyMs FG latency in ms
        /// @param frameTimeMs measured frame time in ms
        /// @param uptime uptime in seconds
        void update(const vk::Vulkan& vk,
            float realFPS, float outputFPS,
            float nativeLatencyMs, float fgLatencyMs,
            float frameTimeMs, float uptime);

        /// Composite the overlay onto a swapchain image.
        /// Adds copy-to-image + blit commands to the given command buffer.
        /// @param vk vulkan instance
        /// @param cmd command buffer (must be in recording state)
        /// @param dstImage destination swapchain image handle
        /// @param dstExtent destination image extent
        /// @param dstOldLayout current layout of the destination region (before blit)
        void render(const vk::Vulkan& vk, VkCommandBuffer cmd,
            VkImage dstImage, VkExtent2D dstExtent,
            VkImageLayout dstOldLayout) const;

        /// Check if the overlay is enabled via toggle file.
        /// @return true if the overlay should be shown
        static bool isEnabled();

        /// Get the overlay image extent.
        VkExtent2D getExtent() const { return extent_; }

    private:
        /// Render text into the CPU pixel buffer.
        void renderTextToBuffer();

        /// Upload the CPU pixel buffer to the GPU image.
        void uploadToGPU(const vk::Vulkan& vk);

        static constexpr uint32_t OVERLAY_WIDTH = 320;
        static constexpr uint32_t OVERLAY_HEIGHT = 130;
        static constexpr uint32_t FONT_CHAR_WIDTH = 6;
        static constexpr uint32_t FONT_CHAR_HEIGHT = 9;
        static constexpr uint32_t PADDING = 8;
        static constexpr uint32_t LINE_HEIGHT = FONT_CHAR_HEIGHT + 3;

        VkExtent2D extent_{OVERLAY_WIDTH, OVERLAY_HEIGHT};

        // CPU-side pixel buffer (RGBA)
        std::vector<uint8_t> pixels_;

        // Current metrics for display
        std::string profileName_;
        uint32_t multiplier_;
        float realFPS_{0};
        float outputFPS_{0};
        float nativeLatencyMs_{0};
        float fgLatencyMs_{0};
        float frameTimeMs_{0};
        float uptime_{0};

        // Vulkan resources
        std::optional<vk::Image> image_;
        std::optional<vk::Buffer> stagingBuffer_;
        bool needsUpload_{false};

        // Persistent mapped pointer for staging buffer
        VkDeviceMemory stagingMemory_{VK_NULL_HANDLE};
        void* mappedPtr_{nullptr};
        VkDevice device_{VK_NULL_HANDLE};
    };

}
