/*
    This file is part of Kdenlive / the FrameForge Pro development branch
    SPDX-FileCopyrightText: 2026 FrameForge Pro contributors

    SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL
*/

#include "frameforge/video/twirl.h"

#include "frameforge/video/sample.h"

#include <algorithm>
#include <cmath>

namespace
{
constexpr double kPi = 3.14159265358979323846;
}

namespace FrameForge::Video
{

Twirl::Twirl() = default;

std::string_view Twirl::id() const
{
    return "frameforge.twirl";
}

std::string_view Twirl::version() const
{
    return "1.0";
}

std::vector<EffectParameterDescriptor> Twirl::parameters() const
{
    return {
        {"angle", 90.0, -720.0, 720.0}, ///< Rotation at the center (degrees)
        {"radius", 100.0, 1.0, 4096.0}, ///< Twirl radius (pixels)
    };
}

void Twirl::setParameter(const std::string &name, double value)
{
    // Clamp to the declared ranges (host bugs must not produce nonsense).
    const auto descriptors = parameters();
    for (const auto &descriptor : descriptors) {
        if (descriptor.name == name) {
            value = std::clamp(value, descriptor.minimum, descriptor.maximum);
            break;
        }
    }
    if (name == "angle") {
        m_angle = value;
    } else if (name == "radius") {
        m_radius = value;
    } else {
        // Unknown parameter: ignored per the IVideoEffect contract.
    }
}

double Twirl::parameter(const std::string &name) const
{
    if (name == "angle") {
        return m_angle;
    }
    if (name == "radius") {
        return m_radius;
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

void Twirl::prepare(int width, int height)
{
    // Packed copy of the frame for the inverse remap: sized here
    // (allocation is allowed in prepare, never in processFrame).
    m_copy.assign(static_cast<size_t>(std::max(1, width)) * 4 * static_cast<size_t>(std::max(1, height)), 0);
}

void Twirl::processFrame(uint8_t *rgba, size_t stride, int width, int height)
{
    // No allocation, no locking: render path (see IVideoEffect contract).
    if (rgba == nullptr || width <= 0 || height <= 0 || stride < static_cast<size_t>(width) * 4) {
        return;
    }
    if (std::abs(m_angle) < 1.0e-9) {
        return; // Identity: no rotation.
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
    const double radius = std::max(1.0, m_radius);
    const double angleRad = m_angle * kPi / 180.0;
    for (int y = 0; y < height; ++y) {
        uint8_t *row = rgba + y * stride;
        const double dy = static_cast<double>(y) - cy;
        for (int x = 0; x < width; ++x) {
            const double dx = static_cast<double>(x) - cx;
            const double d = std::hypot(dx, dy);
            if (d >= radius || d <= 0.0) {
                continue; // Outside the twirl radius (or the center itself).
            }
            // Inverse mapping: rotate the offset back by the local angle.
            const double falloff = 1.0 - d / radius;
            const double theta = -angleRad * falloff;
            const double cosT = std::cos(theta);
            const double sinT = std::sin(theta);
            const double sx = cx + dx * cosT - dy * sinT;
            const double sy = cy + dx * sinT + dy * cosT;
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
