/*
    This file is part of Kdenlive / the FrameForge Pro development branch
    SPDX-FileCopyrightText: 2026 FrameForge Pro contributors

    SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL
*/

#include "frameforge/video/lensflare.h"

#include <algorithm>
#include <cmath>

namespace FrameForge::Video
{

LensFlare::LensFlare() = default;

std::string_view LensFlare::id() const
{
    return "frameforge.lensflare";
}

std::string_view LensFlare::version() const
{
    return "1.0";
}

std::vector<EffectParameterDescriptor> LensFlare::parameters() const
{
    return {
        {"center_x", 50.0, 0.0, 100.0},   ///< Flare center x (percent)
        {"center_y", 50.0, 0.0, 100.0},   ///< Flare center y (percent)
        {"brightness", 100.0, 0.0, 300.0}, ///< Flare strength (percent)
        {"size", 100.0, 1.0, 1024.0},      ///< Flare radius (pixels)
    };
}

void LensFlare::setParameter(const std::string &name, double value)
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
    } else if (name == "brightness") {
        m_brightness = value;
    } else if (name == "size") {
        m_size = value;
    } else {
        // Unknown parameter: ignored per the IVideoEffect contract.
    }
}

double LensFlare::parameter(const std::string &name) const
{
    if (name == "center_x") {
        return m_centerX;
    }
    if (name == "center_y") {
        return m_centerY;
    }
    if (name == "brightness") {
        return m_brightness;
    }
    if (name == "size") {
        return m_size;
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

void LensFlare::prepare(int width, int height)
{
    // The flare is computed from parameters only; nothing to reset.
    (void)width;
    (void)height;
}

void LensFlare::processFrame(uint8_t *rgba, size_t stride, int width, int height)
{
    // No allocation, no locking: render path (see IVideoEffect contract).
    if (rgba == nullptr || width <= 0 || height <= 0 || stride < static_cast<size_t>(width) * 4) {
        return;
    }
    const double brightness = std::clamp(m_brightness, 0.0, 300.0) / 100.0;
    if (brightness <= 0.0) {
        return;
    }
    const double fx = (m_centerX / 100.0) * (width - 1);
    const double fy = (m_centerY / 100.0) * (height - 1);
    const double frameCx = (width - 1) / 2.0;
    const double frameCy = (height - 1) / 2.0;
    // Classic lens reflection: a ghost mirrored through the frame center.
    const double ghostX = 2.0 * frameCx - fx;
    const double ghostY = 2.0 * frameCy - fy;
    const double size = std::max(1.0, m_size);
    const double ringRadius = 0.6 * size;
    const double ringWidth = std::max(1.0, 0.08 * size);
    const double ghostRadius = 0.25 * size;
    // Bounding boxes keep the additive loops tight.
    auto forBoundingBox = [&](double cx, double cy, double radius, auto &&paint) {
        const int x0 = std::max(0, static_cast<int>(std::floor(cx - radius)));
        const int x1 = std::min(width - 1, static_cast<int>(std::ceil(cx + radius)));
        const int y0 = std::max(0, static_cast<int>(std::floor(cy - radius)));
        const int y1 = std::min(height - 1, static_cast<int>(std::ceil(cy + radius)));
        for (int y = y0; y <= y1; ++y) {
            uint8_t *row = rgba + y * stride;
            for (int x = x0; x <= x1; ++x) {
                const double d = std::hypot(static_cast<double>(x) - cx, static_cast<double>(y) - cy);
                uint8_t *pixel = row + static_cast<size_t>(x) * 4;
                paint(pixel, d);
            }
        }
    };
    // Core glow: smooth quadratic falloff at the flare center.
    forBoundingBox(fx, fy, size, [&](uint8_t *pixel, double d) {
        const double core = std::pow(std::max(0.0, 1.0 - d / size), 2.0) * 0.5 * brightness;
        // Halo ring at 60% of the size.
        const double ring = std::max(0.0, 1.0 - std::abs(d - ringRadius) / ringWidth) * 0.3 * brightness;
        const double add = (core + ring) * 255.0;
        if (add > 0.5) {
            for (int c = 0; c < 3; ++c) {
                pixel[c] = static_cast<uint8_t>(std::clamp(pixel[c] + std::lround(add), 0L, 255L));
            }
        }
    });
    // Secondary ghost blob (additive).
    forBoundingBox(ghostX, ghostY, ghostRadius, [&](uint8_t *pixel, double d) {
        const double ghost = std::pow(std::max(0.0, 1.0 - d / ghostRadius), 2.0) * 0.25 * brightness;
        const double add = ghost * 255.0;
        if (add > 0.5) {
            for (int c = 0; c < 3; ++c) {
                pixel[c] = static_cast<uint8_t>(std::clamp(pixel[c] + std::lround(add), 0L, 255L));
            }
        }
    });
}

} // namespace FrameForge::Video
