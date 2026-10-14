/*
    This file is part of Kdenlive / the FrameForge Pro development branch
    SPDX-FileCopyrightText: 2026 FrameForge Pro contributors

    SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL
*/

#include "frameforge/video/lightning.h"

#include <algorithm>
#include <cmath>
#include <cstdint>

namespace
{
constexpr double kPiDouble = 3.14159265358979323846;

/// Deterministic 32-bit hash in the half-open range 0..1 (same scheme as
/// RoughenEdges: uint32 arithmetic, no global state, no UB).
double boltHash(uint32_t ix, uint32_t iy, uint32_t seed)
{
    uint32_t h = ix * 374761393U + iy * 668265263U + seed * 69069U;
    h = (h ^ (h >> 13)) * 1274126177U;
    h = h ^ (h >> 16);
    return static_cast<double>(h & 0xFFFFFFU) / 16777215.0;
}
} // namespace

namespace FrameForge::Video
{

Lightning::Lightning() = default;

std::string_view Lightning::id() const
{
    return "frameforge.lightning";
}

std::string_view Lightning::version() const
{
    return "1.0";
}

std::vector<EffectParameterDescriptor> Lightning::parameters() const
{
    return {
        {"x1", 25.0, 0.0, 100.0},     ///< Start point x (percent)
        {"y1", 25.0, 0.0, 100.0},     ///< Start point y (percent)
        {"x2", 75.0, 0.0, 100.0},     ///< End point x (percent)
        {"y2", 75.0, 0.0, 100.0},     ///< End point y (percent)
        {"amplitude", 30.0, 1.0, 200.0}, ///< Max displacement (pixels)
        {"segments", 12.0, 1.0, 128.0},  ///< Jag resolution
        {"seed", 0.0, 0.0, 1000.0},      ///< Shape evolution (animate for motion)
    };
}

void Lightning::setParameter(const std::string &name, double value)
{
    // Clamp to the declared ranges (host bugs must not produce nonsense).
    const auto descriptors = parameters();
    for (const auto &descriptor : descriptors) {
        if (descriptor.name == name) {
            value = std::clamp(value, descriptor.minimum, descriptor.maximum);
            break;
        }
    }
    if (name == "x1") {
        m_x1 = value;
    } else if (name == "y1") {
        m_y1 = value;
    } else if (name == "x2") {
        m_x2 = value;
    } else if (name == "y2") {
        m_y2 = value;
    } else if (name == "amplitude") {
        m_amplitude = value;
    } else if (name == "segments") {
        m_segments = value;
    } else if (name == "seed") {
        m_seed = value;
    } else {
        // Unknown parameter: ignored per the IVideoEffect contract.
    }
}

double Lightning::parameter(const std::string &name) const
{
    const struct {
        const char *name;
        double value;
    } values[] = {
        {"x1", m_x1}, {"y1", m_y1}, {"x2", m_x2}, {"y2", m_y2},
        {"amplitude", m_amplitude}, {"segments", m_segments}, {"seed", m_seed},
    };
    for (const auto &entry : values) {
        if (name == entry.name) {
            return entry.value;
        }
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

void Lightning::prepare(int width, int height)
{
    // The bolt is derived from parameters only; nothing to reset.
    (void)width;
    (void)height;
}

void Lightning::processFrame(uint8_t *rgba, size_t stride, int width, int height)
{
    // No allocation, no locking: render path (see IVideoEffect contract).
    if (rgba == nullptr || width <= 0 || height <= 0 || stride < static_cast<size_t>(width) * 4) {
        return;
    }
    const double startX = (m_x1 / 100.0) * (width - 1);
    const double startY = (m_y1 / 100.0) * (height - 1);
    const double endX = (m_x2 / 100.0) * (width - 1);
    const double endY = (m_y2 / 100.0) * (height - 1);
    const double dx = endX - startX;
    const double dy = endY - startY;
    const double length = std::hypot(dx, dy);
    if (length < 1.0) {
        return; // Degenerate bolt.
    }
    // Unit vector perpendicular to the bolt axis (displacement direction).
    const double perpX = -dy / length;
    const double perpY = dx / length;
    const int segments = std::max(1, static_cast<int>(std::lround(m_segments)));
    const auto seed = static_cast<uint32_t>(std::lround(m_seed * 1024.0));
    // Bolt vertex at parametric position t in 0..1: point on the axis plus
    // a noise displacement, tapered to zero at both endpoints.
    const auto vertex = [&](int index) {
        const double t = static_cast<double>(index) / segments;
        const double noise = boltHash(static_cast<uint32_t>(index * 31U), 0U, seed) - 0.5;
        const double taper = std::sin(kPiDouble * t);
        const double offset = noise * 2.0 * m_amplitude * taper;
        return std::pair<double, double>{startX + dx * t + perpX * offset, startY + dy * t + perpY * offset};
    };
    // Draw each segment with a DDA line walker (additive white core).
    auto drawPixel = [&](int x, int y) {
        if (x < 0 || y < 0 || x >= width || y >= height) {
            return;
        }
        uint8_t *pixel = rgba + static_cast<size_t>(y) * stride + static_cast<size_t>(x) * 4;
        for (int c = 0; c < 3; ++c) {
            pixel[c] = static_cast<uint8_t>(std::clamp(pixel[c] + 255, 0, 255)); // clamp to int range via values
        }
    };
    for (int i = 0; i < segments; ++i) {
        const auto [ax, ay] = vertex(i);
        const auto [bx, by] = vertex(i + 1);
        const int steps = std::max(1, static_cast<int>(std::lround(std::max(std::abs(bx - ax), std::abs(by - ay)))));
        for (int s = 0; s <= steps; ++s) {
            const double t = static_cast<double>(s) / steps;
            drawPixel(static_cast<int>(std::lround(ax + (bx - ax) * t)), static_cast<int>(std::lround(ay + (by - ay) * t)));
        }
    }
}

} // namespace FrameForge::Video
