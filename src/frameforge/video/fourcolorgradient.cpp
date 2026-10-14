/*
    This file is part of Kdenlive / the FrameForge Pro development branch
    SPDX-FileCopyrightText: 2026 FrameForge Pro contributors

    SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL
*/

#include "frameforge/video/fourcolorgradient.h"

#include <algorithm>
#include <cmath>
#include <cstdint>

namespace
{
void unpackColor(double packed, double out[3])
{
    const auto rgb = static_cast<uint32_t>(std::lround(packed));
    out[0] = ((rgb >> 16) & 0xFF) / 255.0;
    out[1] = ((rgb >> 8) & 0xFF) / 255.0;
    out[2] = (rgb & 0xFF) / 255.0;
}
} // namespace

namespace FrameForge::Video
{

FourColorGradient::FourColorGradient() = default;

std::string_view FourColorGradient::id() const
{
    return "frameforge.fourcolorgradient";
}

std::string_view FourColorGradient::version() const
{
    return "1.0";
}

std::vector<EffectParameterDescriptor> FourColorGradient::parameters() const
{
    const double posMin = 0.0, posMax = 100.0;
    const double colorMax = 16777215.0; // 0xFFFFFF
    return {
        {"x1", 0.0, posMin, posMax},       {"y1", 0.0, posMin, posMax},
        {"x2", 100.0, posMin, posMax},     {"y2", 0.0, posMin, posMax},
        {"x3", 0.0, posMin, posMax},       {"y3", 100.0, posMin, posMax},
        {"x4", 100.0, posMin, posMax},     {"y4", 100.0, posMin, posMax},
        {"color1", 0xFF0000, 0.0, colorMax}, ///< red
        {"color2", 0x00FF00, 0.0, colorMax}, ///< green
        {"color3", 0xFFFF00, 0.0, colorMax}, ///< yellow
        {"color4", 0x0000FF, 0.0, colorMax}, ///< blue
    };
}

void FourColorGradient::setParameter(const std::string &name, double value)
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
    } else if (name == "x3") {
        m_x3 = value;
    } else if (name == "y3") {
        m_y3 = value;
    } else if (name == "x4") {
        m_x4 = value;
    } else if (name == "y4") {
        m_y4 = value;
    } else if (name == "color1") {
        m_color1 = value;
    } else if (name == "color2") {
        m_color2 = value;
    } else if (name == "color3") {
        m_color3 = value;
    } else if (name == "color4") {
        m_color4 = value;
    } else {
        // Unknown parameter: ignored per the IVideoEffect contract.
    }
}

double FourColorGradient::parameter(const std::string &name) const
{
    const struct {
        const char *name;
        double value;
    } values[] = {
        {"x1", m_x1}, {"y1", m_y1}, {"x2", m_x2}, {"y2", m_y2},
        {"x3", m_x3}, {"y3", m_y3}, {"x4", m_x4}, {"y4", m_y4},
        {"color1", m_color1}, {"color2", m_color2}, {"color3", m_color3}, {"color4", m_color4},
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

void FourColorGradient::prepare(int width, int height)
{
    // The gradient does not depend on internal state; nothing to reset.
    (void)width;
    (void)height;
}

void FourColorGradient::processFrame(uint8_t *rgba, size_t stride, int width, int height)
{
    // No allocation, no locking: render path (see IVideoEffect contract).
    if (rgba == nullptr || width <= 0 || height <= 0 || stride < static_cast<size_t>(width) * 4) {
        return;
    }
    // Effect-point positions in pixels.
    const double px[4] = {(m_x1 / 100.0) * (width - 1), (m_x2 / 100.0) * (width - 1), (m_x3 / 100.0) * (width - 1),
                          (m_x4 / 100.0) * (width - 1)};
    const double py[4] = {(m_y1 / 100.0) * (height - 1), (m_y2 / 100.0) * (height - 1), (m_y3 / 100.0) * (height - 1),
                          (m_y4 / 100.0) * (height - 1)};
    double colors[4][3];
    unpackColor(m_color1, colors[0]);
    unpackColor(m_color2, colors[1]);
    unpackColor(m_color3, colors[2]);
    unpackColor(m_color4, colors[3]);
    for (int y = 0; y < height; ++y) {
        uint8_t *row = rgba + y * stride;
        const double fy = static_cast<double>(y);
        for (int x = 0; x < width; ++x) {
            const double fx = static_cast<double>(x);
            // Normalized inverse-square blend of the four colors; the
            // closest point dominates (the documented solid-color
            // circles), with a smooth merge in between.
            double w[4];
            double sum = 0.0;
            for (int i = 0; i < 4; ++i) {
                const double dx = fx - px[i];
                const double dy = fy - py[i];
                const double dist2 = std::max(dx * dx + dy * dy, 1.0e-6);
                w[i] = 1.0 / dist2;
                sum += w[i];
            }
            double rgb[3] = {0.0, 0.0, 0.0};
            for (int i = 0; i < 4; ++i) {
                const double weight = w[i] / sum;
                rgb[0] += weight * colors[i][0];
                rgb[1] += weight * colors[i][1];
                rgb[2] += weight * colors[i][2];
            }
            uint8_t *pixel = row + static_cast<size_t>(x) * 4;
            pixel[0] = static_cast<uint8_t>(std::clamp(std::lround(rgb[0] * 255.0), 0L, 255L));
            pixel[1] = static_cast<uint8_t>(std::clamp(std::lround(rgb[1] * 255.0), 0L, 255L));
            pixel[2] = static_cast<uint8_t>(std::clamp(std::lround(rgb[2] * 255.0), 0L, 255L));
            pixel[3] = 255; // The gradient fills the frame.
        }
    }
}

} // namespace FrameForge::Video
