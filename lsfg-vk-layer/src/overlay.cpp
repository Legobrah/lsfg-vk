/* SPDX-License-Identifier: GPL-3.0-or-later */

#include "overlay.hpp"
#include "lsfg-vk-common/helpers/errors.hpp"
#include "lsfg-vk-common/helpers/pointers.hpp"
#include "lsfg-vk-common/vulkan/command_buffer.hpp"

#include <algorithm>
#include <bitset>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <optional>
#include <sstream>
#include <string>
#include <vector>

#include <vulkan/vulkan_core.h>

using namespace lsfgvk;
using namespace lsfgvk::layer;

// ---------------------------------------------------------------------------
// 5x8 bitmap font (printable ASCII 32-126)
// Each character is 5 columns x 8 rows, packed into 8 bytes (one per row).
// Bit 4 = leftmost pixel, Bit 0 = rightmost pixel.
// ---------------------------------------------------------------------------
static const uint8_t FONT_5x8[][8] = {
    // 32 ' ' (space)
    {0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00},
    // 33 '!'
    {0x04,0x04,0x04,0x04,0x04,0x00,0x04,0x00},
    // 34 '"'
    {0x0A,0x0A,0x00,0x00,0x00,0x00,0x00,0x00},
    // 35 '#'
    {0x0A,0x0A,0x1F,0x0A,0x1F,0x0A,0x0A,0x00},
    // 36 '$'
    {0x04,0x0F,0x14,0x0E,0x05,0x1E,0x04,0x00},
    // 37 '%'
    {0x18,0x19,0x02,0x04,0x08,0x13,0x03,0x00},
    // 38 '&'
    {0x08,0x14,0x14,0x08,0x15,0x12,0x0D,0x00},
    // 39 '''
    {0x04,0x04,0x00,0x00,0x00,0x00,0x00,0x00},
    // 40 '('
    {0x02,0x04,0x08,0x08,0x08,0x04,0x02,0x00},
    // 41 ')'
    {0x08,0x04,0x02,0x02,0x02,0x04,0x08,0x00},
    // 42 '*'
    {0x00,0x04,0x15,0x0E,0x15,0x04,0x00,0x00},
    // 43 '+'
    {0x00,0x04,0x04,0x1F,0x04,0x04,0x00,0x00},
    // 44 ','
    {0x00,0x00,0x00,0x00,0x00,0x04,0x04,0x08},
    // 45 '-'
    {0x00,0x00,0x00,0x1F,0x00,0x00,0x00,0x00},
    // 46 '.'
    {0x00,0x00,0x00,0x00,0x00,0x00,0x04,0x00},
    // 47 '/'
    {0x01,0x01,0x02,0x04,0x08,0x10,0x10,0x00},
    // 48-57 '0'-'9'
    {0x0E,0x11,0x13,0x15,0x19,0x11,0x0E,0x00},
    {0x04,0x0C,0x04,0x04,0x04,0x04,0x0E,0x00},
    {0x0E,0x11,0x01,0x06,0x08,0x10,0x1F,0x00},
    {0x0E,0x11,0x01,0x06,0x01,0x11,0x0E,0x00},
    {0x02,0x06,0x0A,0x12,0x1F,0x02,0x02,0x00},
    {0x1F,0x10,0x1E,0x01,0x01,0x11,0x0E,0x00},
    {0x06,0x08,0x10,0x1E,0x11,0x11,0x0E,0x00},
    {0x1F,0x01,0x02,0x04,0x08,0x08,0x08,0x00},
    {0x0E,0x11,0x11,0x0E,0x11,0x11,0x0E,0x00},
    {0x0E,0x11,0x11,0x0F,0x01,0x02,0x0C,0x00},
    // 58 ':'
    {0x00,0x00,0x04,0x00,0x00,0x04,0x00,0x00},
    // 59 ';'
    {0x00,0x00,0x04,0x00,0x00,0x04,0x04,0x08},
    // 60 '<'
    {0x02,0x04,0x08,0x10,0x08,0x04,0x02,0x00},
    // 61 '='
    {0x00,0x00,0x1F,0x00,0x1F,0x00,0x00,0x00},
    // 62 '>'
    {0x08,0x04,0x02,0x01,0x02,0x04,0x08,0x00},
    // 63 '?'
    {0x0E,0x11,0x01,0x02,0x04,0x00,0x04,0x00},
    // 64 '@'
    {0x0E,0x11,0x17,0x15,0x17,0x10,0x0E,0x00},
    // 65-90 'A'-'Z'
    {0x0E,0x11,0x11,0x1F,0x11,0x11,0x11,0x00},
    {0x1E,0x11,0x11,0x1E,0x11,0x11,0x1E,0x00},
    {0x07,0x08,0x10,0x10,0x10,0x08,0x07,0x00},
    {0x1C,0x12,0x11,0x11,0x11,0x12,0x1C,0x00},
    {0x1F,0x10,0x10,0x1E,0x10,0x10,0x1F,0x00},
    {0x1F,0x10,0x10,0x1E,0x10,0x10,0x10,0x00},
    {0x07,0x08,0x10,0x17,0x11,0x11,0x0F,0x00},
    {0x11,0x11,0x11,0x1F,0x11,0x11,0x11,0x00},
    {0x0E,0x04,0x04,0x04,0x04,0x04,0x0E,0x00},
    {0x01,0x01,0x01,0x01,0x01,0x11,0x0E,0x00},
    {0x11,0x12,0x14,0x18,0x14,0x12,0x11,0x00},
    {0x10,0x10,0x10,0x10,0x10,0x10,0x1F,0x00},
    {0x11,0x1B,0x15,0x15,0x11,0x11,0x11,0x00},
    {0x11,0x19,0x15,0x13,0x11,0x11,0x11,0x00},
    {0x0E,0x11,0x11,0x11,0x11,0x11,0x0E,0x00},
    {0x1E,0x11,0x11,0x1E,0x10,0x10,0x10,0x00},
    {0x0E,0x11,0x11,0x11,0x15,0x12,0x0D,0x00},
    {0x1E,0x11,0x11,0x1E,0x14,0x12,0x11,0x00},
    {0x0E,0x11,0x10,0x0E,0x01,0x11,0x0E,0x00},
    {0x1F,0x04,0x04,0x04,0x04,0x04,0x04,0x00},
    {0x11,0x11,0x11,0x11,0x11,0x11,0x0E,0x00},
    {0x11,0x11,0x11,0x11,0x0A,0x0A,0x04,0x00},
    {0x11,0x11,0x11,0x15,0x15,0x1B,0x11,0x00},
    {0x11,0x11,0x0A,0x04,0x0A,0x11,0x11,0x00},
    {0x11,0x11,0x0A,0x04,0x04,0x04,0x04,0x00},
    {0x1F,0x01,0x02,0x04,0x08,0x10,0x1F,0x00},
    // 91 '['
    {0x0E,0x08,0x08,0x08,0x08,0x08,0x0E,0x00},
    // 92 '\'
    {0x10,0x10,0x08,0x04,0x02,0x01,0x01,0x00},
    // 93 ']'
    {0x0E,0x02,0x02,0x02,0x02,0x02,0x0E,0x00},
    // 94 '^'
    {0x04,0x0A,0x11,0x00,0x00,0x00,0x00,0x00},
    // 95 '_'
    {0x00,0x00,0x00,0x00,0x00,0x00,0x1F,0x00},
    // 96 '`'
    {0x08,0x04,0x00,0x00,0x00,0x00,0x00,0x00},
    // 97-122 'a'-'z'
    {0x00,0x00,0x0E,0x01,0x0F,0x11,0x0F,0x00},
    {0x10,0x10,0x1E,0x11,0x11,0x11,0x1E,0x00},
    {0x00,0x00,0x0E,0x10,0x10,0x10,0x0E,0x00},
    {0x01,0x01,0x0F,0x11,0x11,0x11,0x0F,0x00},
    {0x00,0x00,0x0E,0x11,0x1F,0x10,0x0E,0x00},
    {0x06,0x09,0x08,0x1E,0x08,0x08,0x08,0x00},
    {0x00,0x00,0x0F,0x11,0x0F,0x01,0x0E,0x00},
    {0x10,0x10,0x1E,0x11,0x11,0x11,0x11,0x00},
    {0x04,0x00,0x0C,0x04,0x04,0x04,0x0E,0x00},
    {0x02,0x00,0x06,0x02,0x02,0x12,0x0C,0x00},
    {0x10,0x10,0x12,0x14,0x18,0x14,0x12,0x00},
    {0x0C,0x04,0x04,0x04,0x04,0x04,0x0E,0x00},
    {0x00,0x00,0x1A,0x15,0x15,0x15,0x15,0x00},
    {0x00,0x00,0x1E,0x11,0x11,0x11,0x11,0x00},
    {0x00,0x00,0x0E,0x11,0x11,0x11,0x0E,0x00},
    {0x00,0x00,0x1E,0x11,0x11,0x1E,0x10,0x10},
    {0x00,0x00,0x0F,0x11,0x11,0x0F,0x01,0x01},
    {0x00,0x00,0x16,0x19,0x10,0x10,0x10,0x00},
    {0x00,0x00,0x0F,0x10,0x0E,0x01,0x1E,0x00},
    {0x08,0x08,0x1E,0x08,0x08,0x09,0x06,0x00},
    {0x00,0x00,0x11,0x11,0x11,0x13,0x0D,0x00},
    {0x00,0x00,0x11,0x11,0x0A,0x0A,0x04,0x00},
    {0x00,0x00,0x11,0x15,0x15,0x15,0x0A,0x00},
    {0x00,0x00,0x11,0x0A,0x04,0x0A,0x11,0x00},
    {0x00,0x00,0x11,0x11,0x0F,0x01,0x0E,0x00},
    {0x00,0x00,0x1F,0x02,0x04,0x08,0x1F,0x00},
    // 123 '{'
    {0x02,0x04,0x04,0x08,0x04,0x04,0x02,0x00},
    // 124 '|'
    {0x04,0x04,0x04,0x04,0x04,0x04,0x04,0x00},
    // 125 '}'
    {0x08,0x04,0x04,0x02,0x04,0x04,0x08,0x00},
    // 126 '~'
    {0x00,0x00,0x08,0x15,0x02,0x00,0x00,0x00},
};

/// Get the font bitmap for a character. Returns nullptr for unsupported chars.
static const uint8_t* getCharBitmap(char c) {
    if (c < 32 || c > 126) return nullptr;
    return FONT_5x8[c - 32];
}

// ---------------------------------------------------------------------------
// Overlay implementation
// ---------------------------------------------------------------------------

namespace {
    /// Create a host-visible staging buffer with persistent mapped memory.
    ls::owned_ptr<VkBuffer> createStagingBuffer(const vk::Vulkan& vk, size_t size) {
        VkBuffer handle{};
        const VkBufferCreateInfo info{
            .sType = VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,
            .size = size,
            .usage = VK_BUFFER_USAGE_TRANSFER_SRC_BIT,
            .sharingMode = VK_SHARING_MODE_EXCLUSIVE
        };
        auto res = vk.df().CreateBuffer(vk.dev(), &info, VK_NULL_HANDLE, &handle);
        if (res != VK_SUCCESS)
            throw ls::vulkan_error(res, "overlay: vkCreateBuffer() failed");

        return ls::owned_ptr<VkBuffer>(
            new VkBuffer(handle),
            [dev = vk.dev(), df = vk.df().DestroyBuffer](VkBuffer& b) {
                df(dev, b, VK_NULL_HANDLE);
            }
        );
    }

    /// Allocate host-visible memory for a buffer and map it persistently.
    ls::owned_ptr<VkDeviceMemory> allocateAndMapBuffer(
            const vk::Vulkan& vk, VkBuffer buffer, size_t size, void** outMapped) {
        VkDeviceMemory handle{};

        VkMemoryRequirements reqs{};
        vk.df().GetBufferMemoryRequirements(vk.dev(), buffer, &reqs);

        auto mti = vk.findMemoryTypeIndex(reqs.memoryTypeBits, true);
        if (!mti.has_value())
            throw ls::vulkan_error("overlay: no host-visible memory type found");

        const VkMemoryAllocateInfo memInfo{
            .sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
            .allocationSize = reqs.size,
            .memoryTypeIndex = *mti
        };
        auto res = vk.df().AllocateMemory(vk.dev(), &memInfo, VK_NULL_HANDLE, &handle);
        if (res != VK_SUCCESS)
            throw ls::vulkan_error(res, "overlay: vkAllocateMemory() failed");

        res = vk.df().BindBufferMemory(vk.dev(), buffer, handle, 0);
        if (res != VK_SUCCESS)
            throw ls::vulkan_error(res, "overlay: vkBindBufferMemory() failed");

        res = vk.df().MapMemory(vk.dev(), handle, 0, size, 0, outMapped);
        if (res != VK_SUCCESS)
            throw ls::vulkan_error(res, "overlay: vkMapMemory() failed");

        return ls::owned_ptr<VkDeviceMemory>(
            new VkDeviceMemory(handle),
            [dev = vk.dev(), df = vk.df().FreeMemory, mp = outMapped](VkDeviceMemory& m) {
                df(dev, m, VK_NULL_HANDLE);
                *mp = nullptr;
            }
        );
    }
}

Overlay::Overlay(const vk::Vulkan& vk,
        const std::string& profileName,
        uint32_t multiplier)
        : profileName_(profileName)
        , multiplier_(multiplier) {

    // Allocate CPU pixel buffer (RGBA)
    pixels_.resize(OVERLAY_WIDTH * OVERLAY_HEIGHT * 4, 0);

    // Create overlay image (device-local, transfer src + dst)
    image_.emplace(vk,
        VkExtent2D{OVERLAY_WIDTH, OVERLAY_HEIGHT},
        VK_FORMAT_R8G8B8A8_UNORM,
        VK_IMAGE_USAGE_TRANSFER_DST_BIT | VK_IMAGE_USAGE_TRANSFER_SRC_BIT,
        std::nullopt,  // no import
        std::nullopt   // no export (local image)
    );

    // Render initial frame into pixel buffer
    renderTextToBuffer();
}

bool Overlay::isEnabled() {
    // Check toggle file: /tmp/lsfg-vk-osd
    std::ifstream f("/tmp/lsfg-vk-osd");
    if (!f.is_open()) return false;
    char c{};
    f.get(c);
    return c == '1';
}

void Overlay::update(const vk::Vulkan& vk,
        float realFPS, float outputFPS,
        float nativeLatencyMs, float fgLatencyMs,
        float frameTimeMs, float uptime) {
    realFPS_ = realFPS;
    outputFPS_ = outputFPS;
    nativeLatencyMs_ = nativeLatencyMs;
    fgLatencyMs_ = fgLatencyMs;
    frameTimeMs_ = frameTimeMs;
    uptime_ = uptime;

    renderTextToBuffer();
    uploadToGPU(vk);
}

void Overlay::render(const vk::Vulkan& vk, VkCommandBuffer cmd,
        VkImage dstImage, VkExtent2D dstExtent,
        VkImageLayout dstOldLayout) const {
    if (!image_.has_value()) return;

    const auto& overlayImg = image_->handle();

    // Step 1: Transition overlay image to TRANSFER_DST, then copy buffer to image
    // (We'll use a separate immediate submit for the upload since the main
    //  command buffer is already in the middle of rendering)

    // Step 2: Transition overlay image to TRANSFER_SRC
    const VkImageMemoryBarrier preBlitSrc{
        .sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER,
        .srcAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT,
        .dstAccessMask = VK_ACCESS_TRANSFER_READ_BIT,
        .oldLayout = VK_IMAGE_LAYOUT_GENERAL,
        .newLayout = VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
        .srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED,
        .dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED,
        .image = overlayImg,
        .subresourceRange = {
            .aspectMask = VK_IMAGE_ASPECT_COLOR_BIT,
            .levelCount = 1,
            .layerCount = 1
        }
    };

    // Transition destination image region to TRANSFER_DST
    const VkImageMemoryBarrier preBlitDst{
        .sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER,
        .srcAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT,
        .dstAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT,
        .oldLayout = dstOldLayout,
        .newLayout = VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
        .srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED,
        .dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED,
        .image = dstImage,
        .subresourceRange = {
            .aspectMask = VK_IMAGE_ASPECT_COLOR_BIT,
            .levelCount = 1,
            .layerCount = 1
        }
    };

    VkImageMemoryBarrier preBarriers[] = { preBlitSrc, preBlitDst };
    vk.df().CmdPipelineBarrier(cmd,
        VK_PIPELINE_STAGE_TRANSFER_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT,
        0,
        0, VK_NULL_HANDLE,
        0, VK_NULL_HANDLE,
        2, preBarriers
    );

    // Blit overlay onto top-left corner of the destination image
    const VkImageBlit region{
        .srcSubresource = {
            .aspectMask = VK_IMAGE_ASPECT_COLOR_BIT,
            .layerCount = 1
        },
        .srcOffsets = {
            { 0, 0, 0 },
            { static_cast<int32_t>(OVERLAY_WIDTH),
              static_cast<int32_t>(OVERLAY_HEIGHT), 1 }
        },
        .dstSubresource = {
            .aspectMask = VK_IMAGE_ASPECT_COLOR_BIT,
            .layerCount = 1
        },
        .dstOffsets = {
            { 8, 8, 0 },  // 8px margin from top-left
            { 8 + static_cast<int32_t>(OVERLAY_WIDTH),
              8 + static_cast<int32_t>(OVERLAY_HEIGHT), 1 }
        }
    };
    vk.df().CmdBlitImage(cmd,
        overlayImg, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
        dstImage, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
        1, &region,
        VK_FILTER_LINEAR
    );

    // Transition overlay back to GENERAL
    const VkImageMemoryBarrier postOverlay{
        .sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER,
        .srcAccessMask = VK_ACCESS_TRANSFER_READ_BIT,
        .dstAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT,
        .oldLayout = VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
        .newLayout = VK_IMAGE_LAYOUT_GENERAL,
        .srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED,
        .dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED,
        .image = overlayImg,
        .subresourceRange = {
            .aspectMask = VK_IMAGE_ASPECT_COLOR_BIT,
            .levelCount = 1,
            .layerCount = 1
        }
    };

    // Transition destination back to its original layout
    const VkImageMemoryBarrier postDst{
        .sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER,
        .srcAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT,
        .dstAccessMask = VK_ACCESS_MEMORY_READ_BIT,
        .oldLayout = VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
        .newLayout = dstOldLayout,
        .srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED,
        .dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED,
        .image = dstImage,
        .subresourceRange = {
            .aspectMask = VK_IMAGE_ASPECT_COLOR_BIT,
            .levelCount = 1,
            .layerCount = 1
        }
    };

    VkImageMemoryBarrier postBarriers[] = { postOverlay, postDst };
    vk.df().CmdPipelineBarrier(cmd,
        VK_PIPELINE_STAGE_TRANSFER_BIT, VK_PIPELINE_STAGE_BOTTOM_OF_PIPE_BIT,
        0,
        0, VK_NULL_HANDLE,
        0, VK_NULL_HANDLE,
        2, postBarriers
    );
}

void Overlay::renderTextToBuffer() {
    // Clear to semi-transparent dark background (RGBA)
    const uint8_t bgR = 10, bgG = 12, bgB = 30, bgA = 200;
    for (size_t i = 0; i < pixels_.size(); i += 4) {
        pixels_[i + 0] = bgR;
        pixels_[i + 1] = bgG;
        pixels_[i + 2] = bgB;
        pixels_[i + 3] = bgA;
    }

    // Draw a thin cyan border (top and left edges)
    const uint8_t borderR = 0, borderG = 212, borderB = 255, borderA = 180;
    for (uint32_t x = 0; x < OVERLAY_WIDTH; x++) {
        // Top border line
        size_t idx = (0 * OVERLAY_WIDTH + x) * 4;
        pixels_[idx + 0] = borderR;
        pixels_[idx + 1] = borderG;
        pixels_[idx + 2] = borderB;
        pixels_[idx + 3] = borderA;
    }
    for (uint32_t y = 0; y < OVERLAY_HEIGHT; y++) {
        // Left border line
        size_t idx = (y * OVERLAY_WIDTH + 0) * 4;
        pixels_[idx + 0] = borderR;
        pixels_[idx + 1] = borderG;
        pixels_[idx + 2] = borderB;
        pixels_[idx + 3] = borderA;
    }

    // Text color: white (RGBA)
    const uint8_t txtR = 205, txtG = 214, txtB = 244, txtA = 255;
    // Accent color: cyan (for labels)
    const uint8_t accR = 0, accG = 212, accB = 255, accA = 255;

    // Build text lines
    auto fpsStr = std::string("lsfg-vk");
    if (!profileName_.empty()) {
        fpsStr += "  " + profileName_;
    }

    char line2[64], line3[64], line4[64], line5[64];
    std::snprintf(line2, sizeof(line2), "%.0f fps -> %.0f fps  (%dx)",
        realFPS_, outputFPS_, multiplier_);
    std::snprintf(line3, sizeof(line3), "Native Lat:  %.1f ms", nativeLatencyMs_);
    std::snprintf(line4, sizeof(line4), "FG Lat:      %.1f ms", fgLatencyMs_);
    float overhead = (fgLatencyMs_ > nativeLatencyMs_) ? (fgLatencyMs_ - nativeLatencyMs_) : 0.0f;
    std::snprintf(line5, sizeof(line5), "Overhead:    +%.1f ms", overhead);

    // Draw each line
    auto drawText = [&](const std::string& text, uint32_t x, uint32_t y,
                        uint8_t r, uint8_t g, uint8_t b, uint8_t a) {
        for (char c : text) {
            const uint8_t* glyph = getCharBitmap(c);
            if (glyph) {
                for (uint32_t row = 0; row < 8; row++) {
                    uint8_t bits = glyph[row];
                    for (uint32_t col = 0; col < 5; col++) {
                        if (bits & (1 << (4 - col))) {
                            uint32_t px = x + col;
                            uint32_t py = y + row;
                            if (px < OVERLAY_WIDTH && py < OVERLAY_HEIGHT) {
                                size_t idx = (py * OVERLAY_WIDTH + px) * 4;
                                pixels_[idx + 0] = r;
                                pixels_[idx + 1] = g;
                                pixels_[idx + 2] = b;
                                pixels_[idx + 3] = a;
                            }
                        }
                    }
                }
            }
            x += FONT_CHAR_WIDTH;
        }
    };

    uint32_t y = PADDING;
    drawText(fpsStr, PADDING, y, accR, accG, accB, accA);
    y += LINE_HEIGHT + 4;
    drawText(line2, PADDING, y, txtR, txtG, txtB, txtA);
    y += LINE_HEIGHT;
    drawText(line3, PADDING, y, txtR, txtG, txtB, txtA);
    y += LINE_HEIGHT;
    drawText(line4, PADDING, y, txtR, txtG, txtB, txtA);
    y += LINE_HEIGHT;
    drawText(line5, PADDING, y, txtR, txtG, txtB, txtA);
}

void Overlay::uploadToGPU(const vk::Vulkan& vk) {
    if (!image_.has_value()) return;

    // Recreate staging buffer with fresh pixel data each frame.
    // The buffer is small (~166KB) so allocation is cheap.
    const size_t bufSize = OVERLAY_WIDTH * OVERLAY_HEIGHT * 4;
    stagingBuffer_.emplace(vk, pixels_.data(), bufSize,
        VK_BUFFER_USAGE_TRANSFER_SRC_BIT);

    // Create a temporary command buffer for the upload
    vk::CommandBuffer cmd(vk);
    cmd.begin(vk);
    cmd.copyBufferToImage(vk, stagingBuffer_.value(), image_.value());
    cmd.end(vk);
    cmd.submit(vk);
}
