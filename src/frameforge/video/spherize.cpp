/*
    This file is part of Kdenlive / the FrameForge Pro development branch
    SPDX-FileCopyrightText: 2026 FrameForge Pro contributors

    SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL
*/

#include "frameforge/video/spherize.h"

#include "frameforge/video/sample.h"

#include <algorithm>
#include <cmath>

namespace FrameForge::Video
{

Spherize::Spherize() = default;

std::string_view Spherize::id() const
{
    return "frameforge.spherize";
}

std::string_view Spherize::version() const
{
    return "1.0";
}

std::vector<EffectParameterDescriptor> Spherize::parameters() const
{
    return {
        {"amount", 50.0, 0.0, 100.0},      ///< Bulge strength (percent)
        {"radius_percent", 100.0, 0.0, 100.0}, ///< Radius (% of half the smaller dimension)
    };
}

void Spherize::setParameter(const std::string &name, double value)
{
    // Clamp to the declared ranges (host bugs must not produce nonsense).
    const auto descriptors = parameters();
    for (const auto &descriptor : descriptors) {
        if (descriptor.name == name) {
            value = std::clamp(value, descriptor.minimum, descriptor.maximum);
            break;
        }
    }
    if (name == "amount") {
        m_amount = value;
    } else if (name == "radius_percent") {
        m_radiusPercent = value;
    } else {
        // Unknown parameter: ignored per the IVideoEffect contract.
    }
}

double Spherize::parameter(const std::string &name) const
{
    if (name == "amount") {
        return m_amount;
    }
    if (name == "radius_percent") {
        return m_radiusPercent;
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

void Spherize::prepare(int width, int height)
{
    // Packed copy of the frame for the inverse remap: sized here
    // (allocation is allowed in prepare, never in processFrame).
    m_copy.assign(static_cast<size_t>(std::max(1, width)) * 4 * static_cast<size_t>(std::max(1, height)), 0);
}

void Spherize::processFrame(uint8_t *rgba, size_t stride, int width, int height)
{
    // No allocation, no locking: render path (see IVideoEffect contract).
    if (rgba == nullptr || width <= 0 || height <= 0 || stride < static_cast<size_t>(width) * 4) {
        return;
    }
    const double strength = std::clamp(m_amount, 0.0, 100.0) / 100.0;
    if (strength <= 0.0 || m_radiusPercent <= 0.0) {
        return; // Identity: no bulge.
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
    const double cx = (static_cast<double>(width) - 1.0) / 2.0;
    const double cy = (static_cast<double>(height) - 1.0) / 2.0;
    const double radius = (m_radiusPercent / 100.0) * (std::min(width, height) * 0.5);
    if (radius < 1.0) {
        return;
    }
    for (int y = 0; y < height; ++y) {
        uint8_t *row = rgba + y * stride;
        const double dy = static_cast<double>(y) - cy;
        for (int x = 0; x < width; ++x) {
            const double dx = static_cast<double>(x) - cx;
            const double d = std::hypot(dx, dy);
            if (d >= radius || d <= 0.0) {
                continue; // Outside the sphere radius (or the center itself).
            }
            const double nd = d / radius;
            // Inverse mapping: pull the sample toward the center; the
            // factor is 1 at the boundary (seamless) and below 1 inside
            // (center magnification).
            const double factor = 1.0 / (1.0 + strength * (1.0 - nd * nd));
            const double sx = cx + dx * factor;
            const double sy = cy + dy * factor;
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
