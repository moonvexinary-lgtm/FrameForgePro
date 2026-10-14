/*
    This file is part of Kdenlive / the FrameForge Pro development branch
    SPDX-FileCopyrightText: 2026 FrameForge Pro contributors

    SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL
*/

#include "frameforge/video/magnify.h"

#include "frameforge/video/sample.h"

#include <algorithm>
#include <cmath>

namespace FrameForge::Video
{

Magnify::Magnify() = default;

std::string_view Magnify::id() const
{
    return "frameforge.magnify";
}

std::string_view Magnify::version() const
{
    return "1.0";
}

std::vector<EffectParameterDescriptor> Magnify::parameters() const
{
    return {
        {"center_x", 50.0, 0.0, 100.0},      ///< Center x (percent of width)
        {"center_y", 50.0, 0.0, 100.0},      ///< Center y (percent of height)
        {"radius", 100.0, 1.0, 4096.0},       ///< Circle radius (pixels)
        {"magnification", 150.0, 100.0, 1000.0}, ///< Scale (percent)
    };
}

void Magnify::setParameter(const std::string &name, double value)
{
    // Clamp to the declared ranges (host bugs must not produce nonsense).
    const auto descriptors = parameters();
    for (const auto &descriptor : descriptors) {
        if (descriptor.name == name) {
            value = std::clamp(value, descriptor.minimum, descriptor.maximum);
            break;
        }
    }
    if (name == "center_x") {
        m_centerX = value;
    } else if (name == "center_y") {
        m_centerY = value;
    } else if (name == "radius") {
        m_radius = value;
    } else if (name == "magnification") {
        m_magnification = value;
    } else {
        // Unknown parameter: ignored per the IVideoEffect contract.
    }
}

double Magnify::parameter(const std::string &name) const
{
    if (name == "center_x") {
        return m_centerX;
    }
    if (name == "center_y") {
        return m_centerY;
    }
    if (name == "radius") {
        return m_radius;
    }
    if (name == "magnification") {
        return m_magnification;
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

void Magnify::prepare(int width, int height)
{
    // Packed copy of the frame for the inverse remap: sized here
    // (allocation is allowed in prepare, never in processFrame).
    m_copy.assign(static_cast<size_t>(std::max(1, width)) * 4 * static_cast<size_t>(std::max(1, height)), 0);
}

void Magnify::processFrame(uint8_t *rgba, size_t stride, int width, int height)
{
    // No allocation, no locking: render path (see IVideoEffect contract).
    if (rgba == nullptr || width <= 0 || height <= 0 || stride < static_cast<size_t>(width) * 4) {
        return;
    }
    if (m_magnification <= 100.0) {
        return; // Identity (or degenerate): nothing to magnify.
    }
    const size_t packedStride = static_cast<size_t>(width) * 4;
    if (m_copy.size() < packedStride * static_cast<size_t>(height)) {
        return; // Host changed geometry without prepare().
    }
    // Copy the source into the packed scratch (bilinear reads must not
    // see already-remapped pixels).
    for (int y = 0; y < height; ++y) {
        std::copy_n(rgba + y * stride, packedStride, m_copy.data() + y * packedStride);
    }
    const double cx = (m_centerX / 100.0) * (static_cast<double>(width) - 1.0);
    const double cy = (m_centerY / 100.0) * (static_cast<double>(height) - 1.0);
    const double radius = std::max(1.0, m_radius);
    const double scale = 100.0 / m_magnification;
    for (int y = 0; y < height; ++y) {
        uint8_t *row = rgba + y * stride;
        const double dy = static_cast<double>(y) - cy;
        for (int x = 0; x < width; ++x) {
            const double dx = static_cast<double>(x) - cx;
            if (std::hypot(dx, dy) >= radius) {
                continue; // Outside the magnifier circle: untouched.
            }
            const double sx = cx + dx * scale;
            const double sy = cy + dy * scale;
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
