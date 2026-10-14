/*
    This file is part of Kdenlive / the FrameForge Pro development branch
    SPDX-FileCopyrightText: 2026 FrameForge Pro contributors

    SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL
*/

#include "frameforge/video/levels.h"

#include <algorithm>
#include <cmath>

namespace FrameForge::Video
{

Levels::Levels()
{
    rebuildTable();
}

std::string_view Levels::id() const
{
    return "frameforge.levels";
}

std::string_view Levels::version() const
{
    return "1.0";
}

std::vector<EffectParameterDescriptor> Levels::parameters() const
{
    return {
        {"input_black", 0.0, 0.0, 255.0},  ///< Input values at/below map to output black
        {"input_white", 255.0, 0.0, 255.0}, ///< Input values at/above map to output white
        {"gamma", 1.0, 0.1, 10.0},          ///< Midtone response (1.0 = neutral)
        {"output_black", 0.0, 0.0, 255.0},  ///< Black output level
        {"output_white", 255.0, 0.0, 255.0} ///< White output level
    };
}

void Levels::setParameter(const std::string &name, double value)
{
    // Clamp to the declared ranges (host bugs must not produce nonsense).
    const auto descriptors = parameters();
    for (const auto &descriptor : descriptors) {
        if (descriptor.name == name) {
            value = std::clamp(value, descriptor.minimum, descriptor.maximum);
            break;
        }
    }
    if (name == "input_black") {
        m_inputBlack = value;
    } else if (name == "input_white") {
        m_inputWhite = value;
    } else if (name == "gamma") {
        m_gamma = value;
    } else if (name == "output_black") {
        m_outputBlack = value;
    } else if (name == "output_white") {
        m_outputWhite = value;
    } else {
        // Unknown parameter: ignored per the IVideoEffect contract.
        return;
    }
    rebuildTable();
}

double Levels::parameter(const std::string &name) const
{
    if (name == "input_black") {
        return m_inputBlack;
    }
    if (name == "input_white") {
        return m_inputWhite;
    }
    if (name == "gamma") {
        return m_gamma;
    }
    if (name == "output_black") {
        return m_outputBlack;
    }
    if (name == "output_white") {
        return m_outputWhite;
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

void Levels::prepare(int width, int height)
{
    // The LUT does not depend on geometry; nothing to reset. Parameters are
    // validated to the contract by accepting the notification.
    (void)width;
    (void)height;
}

void Levels::processFrame(uint8_t *rgba, size_t stride, int width, int height)
{
    // No allocation, no locking: render path (see IVideoEffect contract).
    if (rgba == nullptr || width <= 0 || height <= 0 || stride < static_cast<size_t>(width) * 4) {
        return;
    }
    for (int y = 0; y < height; ++y) {
        uint8_t *row = rgba + y * stride;
        for (int x = 0; x < width; ++x) {
            uint8_t *pixel = row + static_cast<size_t>(x) * 4;
            pixel[0] = m_lut[pixel[0]];
            pixel[1] = m_lut[pixel[1]];
            pixel[2] = m_lut[pixel[2]];
            // Alpha is passed through untouched.
        }
    }
}

void Levels::rebuildTable()
{
    // Normalize the parameter ranges defensively: the transform requires
    // input_black <= input_white and output_black <= output_white.
    const double inLow = std::min(m_inputBlack, m_inputWhite);
    const double inHigh = std::max(m_inputBlack, m_inputWhite);
    const double outLow = std::min(m_outputBlack, m_outputWhite);
    const double outHigh = std::max(m_outputBlack, m_outputWhite);
    const double inRange = inHigh - inLow;
    const double outRange = outHigh - outLow;
    const double invGamma = 1.0 / std::max(0.1, m_gamma);
    for (int v = 0; v < 256; ++v) {
        double t;
        if (inRange < 1.0) {
            // Degenerate input range: binary threshold at the midpoint
            // (all values below become output black, above become output white).
            t = (static_cast<double>(v) - inLow) >= 0.5 ? 1.0 : 0.0;
        } else {
            t = std::clamp((static_cast<double>(v) - inLow) / inRange, 0.0, 1.0);
            t = std::pow(t, invGamma);
        }
        const double mapped = outLow + t * outRange;
        m_lut[static_cast<size_t>(v)] = static_cast<uint8_t>(std::lround(std::clamp(mapped, 0.0, 255.0)));
    }
}

} // namespace FrameForge::Video
