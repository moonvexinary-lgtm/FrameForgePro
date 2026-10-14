/*
    This file is part of Kdenlive / the FrameForge Pro development branch
    SPDX-FileCopyrightText: 2026 FrameForge Pro contributors

    SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL
*/

#include "frameforge/video/cineon.h"

#include <algorithm>
#include <cmath>

namespace FrameForge::Video
{

namespace
{
constexpr double kLogDecadeCodes = 300.0; ///< Cineon code values per decade (see class documentation).
}

CineonConverter::CineonConverter()
{
    rebuildTable();
}

std::string_view CineonConverter::id() const
{
    return "frameforge.cineonconverter";
}

std::string_view CineonConverter::version() const
{
    return "1.0";
}

std::vector<EffectParameterDescriptor> CineonConverter::parameters() const
{
    return {
        {"conversion", 1.0, 0.0, 1.0},     ///< 1 = Log->Linear, 0 = Linear->Log
        {"black_point", 95.0, 0.0, 1023.0},  ///< 10-bit reference black
        {"white_point", 685.0, 0.0, 1023.0}, ///< 10-bit reference white
        {"gamma", 1.0, 0.1, 10.0},           ///< Linear gamma
    };
}

void CineonConverter::setParameter(const std::string &name, double value)
{
    // Clamp to the declared ranges (host bugs must not produce nonsense).
    const auto descriptors = parameters();
    for (const auto &descriptor : descriptors) {
        if (descriptor.name == name) {
            value = std::clamp(value, descriptor.minimum, descriptor.maximum);
            break;
        }
    }
    if (name == "conversion") {
        m_conversion = value > 0.5 ? 1.0 : 0.0;
    } else if (name == "black_point") {
        m_blackPoint = value;
    } else if (name == "white_point") {
        m_whitePoint = value;
    } else if (name == "gamma") {
        m_gamma = value;
    } else {
        // Unknown parameter: ignored per the IVideoEffect contract.
        return;
    }
    rebuildTable();
}

double CineonConverter::parameter(const std::string &name) const
{
    if (name == "conversion") {
        return m_conversion;
    }
    if (name == "black_point") {
        return m_blackPoint;
    }
    if (name == "white_point") {
        return m_whitePoint;
    }
    if (name == "gamma") {
        return m_gamma;
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

void CineonConverter::prepare(int width, int height)
{
    // The LUT does not depend on geometry; nothing to reset.
    (void)width;
    (void)height;
}

void CineonConverter::processFrame(uint8_t *rgba, size_t stride, int width, int height)
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

void CineonConverter::rebuildTable()
{
    // Normalize the points defensively (black <= white).
    const double black = std::min(m_blackPoint, m_whitePoint);
    const double white = std::max(m_blackPoint, m_whitePoint);
    // Linear value of the reference black on the log segment (the Cineon
    // "toe" anchor: ~0.0108 with the default 95/685 points).
    const double toeValue = std::pow(10.0, (black - white) / kLogDecadeCodes);
    const double invGamma = 1.0 / std::max(0.1, m_gamma);
    const bool logToLinear = m_conversion > 0.5;
    for (int v = 0; v < 256; ++v) {
        double out = 0.0;
        if (logToLinear) {
            // Cineon Log -> Linear: 8-bit frame carries the 10-bit log
            // code domain (v8 * 1023/255).
            const double code = static_cast<double>(v) * (1023.0 / 255.0);
            double linear;
            if (code <= black || toeValue <= 0.0) {
                // Linear toe from 0 to the reference-black log value.
                linear = (black > 0.0) ? (std::max(0.0, code) / black) * toeValue : 0.0;
            } else {
                linear = std::pow(10.0, (code - white) / kLogDecadeCodes);
            }
            out = std::pow(std::max(0.0, linear), invGamma);
        } else {
            // Linear -> Cineon Log: the 8-bit frame carries linear data;
            // the output byte carries the 10-bit log code.
            const double linear = std::pow(static_cast<double>(v) / 255.0, invGamma);
            double code;
            if (linear <= toeValue) {
                code = (toeValue > 0.0) ? (linear / toeValue) * black : 0.0;
            } else {
                code = white + kLogDecadeCodes * std::log10(std::max(1.0e-12, linear));
            }
            out = std::clamp(code, 0.0, 1023.0) / 1023.0;
        }
        m_lut[static_cast<size_t>(v)] = static_cast<uint8_t>(std::lround(std::clamp(out * 255.0, 0.0, 255.0)));
    }
}

} // namespace FrameForge::Video
