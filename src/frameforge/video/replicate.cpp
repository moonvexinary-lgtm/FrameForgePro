/*
    This file is part of Kdenlive / the FrameForge Pro development branch
    SPDX-FileCopyrightText: 2026 FrameForge Pro contributors

    SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL
*/

#include "frameforge/video/replicate.h"

#include "frameforge/video/sample.h"

#include <algorithm>
#include <cmath>

namespace FrameForge::Video
{

Replicate::Replicate() = default;

std::string_view Replicate::id() const
{
    return "frameforge.replicate";
}

std::string_view Replicate::version() const
{
    return "1.0";
}

std::vector<EffectParameterDescriptor> Replicate::parameters() const
{
    return {
        {"horizontal", 2.0, 1.0, 16.0}, ///< Tiles per row
        {"vertical", 2.0, 1.0, 16.0},   ///< Tiles per column
    };
}

void Replicate::setParameter(const std::string &name, double value)
{
    // Clamp to the declared ranges (host bugs must not produce nonsense).
    const auto descriptors = parameters();
    for (const auto &descriptor : descriptors) {
        if (descriptor.name == name) {
            value = std::clamp(value, descriptor.minimum, descriptor.maximum);
            break;
        }
    }
    if (name == "horizontal") {
        m_horizontal = value;
    } else if (name == "vertical") {
        m_vertical = value;
    } else {
        // Unknown parameter: ignored per the IVideoEffect contract.
    }
}

double Replicate::parameter(const std::string &name) const
{
    if (name == "horizontal") {
        return m_horizontal;
    }
    if (name == "vertical") {
        return m_vertical;
    }
    // Unknown parameter: return the declared default per the contract.
    const auto descriptors = parameters();
    for (const auto &descriptor : descriptors) {
        if (descriptor.name == name) {
            return descriptor.defaultValue;
        }
    }
    return 0.0;
}

void Replicate::prepare(int width, int height)
{
    // Packed copy of the frame for the tiling remap: sized here
    // (allocation is allowed in prepare, never in processFrame).
    m_copy.assign(static_cast<size_t>(std::max(1, width)) * 4 * static_cast<size_t>(std::max(1, height)), 0);
}

void Replicate::processFrame(uint8_t *rgba, size_t stride, int width, int height)
{
    // No allocation, no locking: render path (see IVideoEffect contract).
    if (rgba == nullptr || width <= 0 || height <= 0 || stride < static_cast<size_t>(width) * 4) {
        return;
    }
    const int tilesX = std::max(1, static_cast<int>(std::lround(m_horizontal)));
    const int tilesY = std::max(1, static_cast<int>(std::lround(m_vertical)));
    if (tilesX <= 1 && tilesY <= 1) {
        return; // Identity: a single tile shows the whole frame.
    }
    const size_t packedStride = static_cast<size_t>(width) * 4;
    if (m_copy.size() < packedStride * static_cast<size_t>(height)) {
        return; // Host changed geometry without prepare().
    }
    // Copy the source into the packed scratch (tiles must not read
    // already-overwritten pixels).
    for (int y = 0; y < height; ++y) {
        std::copy_n(rgba + y * stride, packedStride, m_copy.data() + y * packedStride);
    }
    const int cellWidth = std::max(1, (width + tilesX - 1) / tilesX);
    const int cellHeight = std::max(1, (height + tilesY - 1) / tilesY);
    const double sourceX = static_cast<double>(width - 1);
    const double sourceY = static_cast<double>(height - 1);
    for (int y = 0; y < height; ++y) {
        uint8_t *row = rgba + y * stride;
        const int localY = y % cellHeight;
        const double sy = (cellHeight > 1) ? (static_cast<double>(localY) / (cellHeight - 1)) * sourceY : 0.0;
        for (int x = 0; x < width; ++x) {
            const int localX = x % cellWidth;
            const double sx = (cellWidth > 1) ? (static_cast<double>(localX) / (cellWidth - 1)) * sourceX : 0.0;
            uint8_t sampled[4];
            sampleBilinear(m_copy.data(), packedStride, width, height, sx, sy, sampled);
            uint8_t *pixel = row + static_cast<size_t>(x) * 4;
            pixel[0] = sampled[0];
            pixel[1] = sampled[1];
            pixel[2] = sampled[2];
            pixel[3] = sampled[3];
        }
    }
}

} // namespace FrameForge::Video
