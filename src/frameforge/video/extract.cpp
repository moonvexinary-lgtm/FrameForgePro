/*
    This file is part of Kdenlive / the FrameForge Pro development branch
    SPDX-FileCopyrightText: 2026 FrameForge Pro contributors

    SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL
*/

#include "frameforge/video/extract.h"

#include <algorithm>
#include <cmath>

namespace FrameForge::Video
{

Extract::Extract()
{
    rebuildTable();
}

std::string_view Extract::id() const
{
    return "frameforge.extract";
}

std::string_view Extract::version() const
{
    return "1.0";
}

std::vector<EffectParameterDescriptor> Extract::parameters() const
{
    return {
        {"black_input", 0.0, 0.0, 255.0},   ///< Black Input Level (luminance)
        {"white_input", 255.0, 0.0, 255.0}, ///< White Input Level (luminance)
        {"softness", 0.0, 0.0, 100.0},       ///< Ramp width around the cutoffs (percent)
        {"invert", 0.0, 0.0, 1.0},           ///< Swap black/white result
    };
}

void Extract::setParameter(const std::string &name, double value)
{
    // Clamp to the declared ranges (host bugs must not produce nonsense).
    const auto descriptors = parameters();
    for (const auto &descriptor : descriptors) {
        if (descriptor.name == name) {
            value = std::clamp(value, descriptor.minimum, descriptor.maximum);
            break;
        }
    }
    if (name == "black_input") {
        m_blackInput = value;
    } else if (name == "white_input") {
        m_whiteInput = value;
    } else if (name == "softness") {
        m_softness = value;
    } else if (name == "invert") {
        m_invert = value > 0.5;
    } else {
        // Unknown parameter: ignored per the IVideoEffect contract.
        return;
    }
    rebuildTable();
}

double Extract::parameter(const std::string &name) const
{
    if (name == "black_input") {
        return m_blackInput;
    }
    if (name == "white_input") {
        return m_whiteInput;
    }
    if (name == "softness") {
        return m_softness;
    }
    if (name == "invert") {
        return m_invert ? 1.0 : 0.0;
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

void Extract::prepare(int width, int height)
{
    // The LUT does not depend on geometry; nothing to reset.
    (void)width;
    (void)height;
}

void Extract::processFrame(uint8_t *rgba, size_t stride, int width, int height)
{
    // No allocation, no locking: render path (see IVideoEffect contract).
    if (rgba == nullptr || width <= 0 || height <= 0 || stride < static_cast<size_t>(width) * 4) {
        return;
    }
    for (int y = 0; y < height; ++y) {
        uint8_t *row = rgba + y * stride;
        for (int x = 0; x < width; ++x) {
            uint8_t *pixel = row + static_cast<size_t>(x) * 4;
            // Rec.709 luminance, rounded into the LUT's domain.
            const double luminance = 0.2126 * pixel[0] + 0.7152 * pixel[1] + 0.0722 * pixel[2];
            const uint8_t index = static_cast<uint8_t>(std::clamp(std::lround(luminance), 0L, 255L));
            const uint8_t mapped = m_lut[index];
            pixel[0] = mapped;
            pixel[1] = mapped;
            pixel[2] = mapped;
            // Alpha is passed through untouched.
        }
    }
}

void Extract::rebuildTable()
{
    // Normalize the levels defensively (black <= white).
    const double black = std::min(m_blackInput, m_whiteInput);
    const double white = std::max(m_blackInput, m_whiteInput);
    const double range = white - black;
    // Softness (percent) widens the cutoffs into linear ramps.
    const double ramp = std::min(m_softness / 100.0, 1.0) * (range > 0.0 ? range * 0.5 : 0.0);
    for (int v = 0; v < 256; ++v) {
        const double luminance = static_cast<double>(v);
        double value = 0.0;
        if (range < 1.0) {
            // Degenerate range: only the exact level survives as white.
            value = std::abs(luminance - black) <= 0.5 ? 1.0 : 0.0;
        } else if (luminance <= black || luminance >= white) {
            // Documented behavior: below Black Input Level or above White
            // Input Level -> black.
            value = 0.0;
        } else {
            // Between the levels -> gray/white, proportional.
            value = (luminance - black) / range;
            if (ramp > 0.0) {
                if (luminance < black + ramp) {
                    value *= (luminance - black) / ramp;
                } else if (luminance > white - ramp) {
                    value *= (white - luminance) / ramp;
                }
            }
        }
        if (m_invert) {
            value = 1.0 - value;
        }
        m_lut[static_cast<size_t>(v)] = static_cast<uint8_t>(std::lround(std::clamp(value, 0.0, 1.0) * 255.0));
    }
}

} // namespace FrameForge::Video
