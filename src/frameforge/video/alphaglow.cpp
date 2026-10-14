/*
    This file is part of Kdenlive / the FrameForge Pro development branch
    SPDX-FileCopyrightText: 2026 FrameForge Pro contributors

    SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL
*/

#include "frameforge/video/alphaglow.h"

#include "frameforge/video/alphablur.h"

#include <algorithm>
#include <cmath>

namespace FrameForge::Video
{

AlphaGlow::AlphaGlow() = default;

std::string_view AlphaGlow::id() const
{
    return "frameforge.alphaglow";
}

std::string_view AlphaGlow::version() const
{
    return "1.0";
}

std::vector<EffectParameterDescriptor> AlphaGlow::parameters() const
{
    return {
        {"glow", 10.0, 0.0, 100.0},          ///< Spread from the alpha edge (px)
        {"brightness", 100.0, 0.0, 100.0},     ///< Glow strength (percent)
        {"color_a", 16777215.0, 0.0, 16777215.0}, ///< Near-edge color, 0xRRGGBB
        {"color_b", 16777215.0, 0.0, 16777215.0}, ///< Outer glow color, 0xRRGGBB
    };
}

void AlphaGlow::setParameter(const std::string &name, double value)
{
    // Clamp to the declared ranges (host bugs must not produce nonsense).
    const auto descriptors = parameters();
    for (const auto &descriptor : descriptors) {
        if (descriptor.name == name) {
            value = std::clamp(value, descriptor.minimum, descriptor.maximum);
            break;
        }
    }
    if (name == "glow") {
        m_glow = value;
    } else if (name == "brightness") {
        m_brightness = value;
    } else if (name == "color_a") {
        m_colorA = value;
    } else if (name == "color_b") {
        m_colorB = value;
    } else {
        // Unknown parameter: ignored per the IVideoEffect contract.
    }
}

double AlphaGlow::parameter(const std::string &name) const
{
    if (name == "glow") {
        return m_glow;
    }
    if (name == "brightness") {
        return m_brightness;
    }
    if (name == "color_a") {
        return m_colorA;
    }
    if (name == "color_b") {
        return m_colorB;
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

void AlphaGlow::prepare(int width, int height)
{
    // Scratch for the blur and the original-alpha copy: sized here
    // (allocation is allowed in prepare, never in processFrame).
    const size_t pixels = static_cast<size_t>(std::max(1, width)) * static_cast<size_t>(std::max(1, height));
    m_temp.assign(pixels, 0.0f);
    m_origAlpha.assign(pixels, 0);
    m_line.assign(static_cast<size_t>(std::max(width, height)) + 1, 0.0f);
}

void AlphaGlow::processFrame(uint8_t *rgba, size_t stride, int width, int height)
{
    // No allocation, no locking: render path (see IVideoEffect contract).
    if (rgba == nullptr || width <= 0 || height <= 0 || stride < static_cast<size_t>(width) * 4) {
        return;
    }
    const size_t pixels = static_cast<size_t>(width) * static_cast<size_t>(height);
    if (m_glow < 1.0 || m_brightness <= 0.0 || m_temp.size() < pixels || m_origAlpha.size() < pixels
        || m_line.size() < static_cast<size_t>(std::max(width, height)) + 1) {
        return;
    }

    // Keep the original alpha: the glow lives in the spread-minus-original
    // region outside the alpha footprint.
    for (int y = 0; y < height; ++y) {
        const uint8_t *row = rgba + y * stride;
        uint8_t *orig = m_origAlpha.data() + static_cast<size_t>(y) * width;
        for (int x = 0; x < width; ++x) {
            orig[x] = row[static_cast<size_t>(x) * 4 + 3];
        }
    }

    // Spread the alpha with the shared box blur (radius = glow in pixels).
    const int radius = static_cast<int>(std::lround(m_glow));
    boxBlurAlpha(rgba, stride, width, height, radius, m_temp.data(), m_line.data());

    // Unpack the glow colors once per frame.
    const auto colorA = static_cast<uint32_t>(std::lround(m_colorA));
    const auto colorB = static_cast<uint32_t>(std::lround(m_colorB));
    const double aR = ((colorA >> 16) & 0xFF) / 255.0;
    const double aG = ((colorA >> 8) & 0xFF) / 255.0;
    const double aB = (colorA & 0xFF) / 255.0;
    const double bR = ((colorB >> 16) & 0xFF) / 255.0;
    const double bG = ((colorB >> 8) & 0xFF) / 255.0;
    const double bB = (colorB & 0xFF) / 255.0;
    const double brightness = m_brightness / 100.0;

    for (int y = 0; y < height; ++y) {
        uint8_t *row = rgba + y * stride;
        const uint8_t *orig = m_origAlpha.data() + static_cast<size_t>(y) * width;
        for (int x = 0; x < width; ++x) {
            uint8_t *pixel = row + static_cast<size_t>(x) * 4;
            const double sourceAlpha = orig[x] / 255.0;
            const double spreadAlpha = pixel[3] / 255.0;
            // Glow mask: the region outside the source alpha.
            double glow = spreadAlpha - sourceAlpha;
            if (glow <= 0.0) {
                // Interior or far outside the spread: untouched.
                continue;
            }
            glow = std::min(glow, 1.0);
            const double glowAlpha = glow * brightness;
            const double outAlpha = sourceAlpha + glowAlpha * (1.0 - sourceAlpha);
            if (outAlpha <= 0.0) {
                continue;
            }
            // Color shifts from color_a (near the edge, glow ~ 1) to color_b
            // at the outer end (glow ~ 0).
            const double fade = 1.0 - glow;
            const double colorR = aR + (bR - aR) * fade;
            const double colorG = aG + (bG - aG) * fade;
            const double colorB2 = aB + (bB - aB) * fade;
            // Over-composite the glow under the original content
            // (straight alpha).
            const double glowWeight = glowAlpha * (1.0 - sourceAlpha);
            pixel[0] = static_cast<uint8_t>(std::clamp(
                std::lround((sourceAlpha * pixel[0] + glowWeight * colorR * 255.0) / outAlpha), 0L, 255L));
            pixel[1] = static_cast<uint8_t>(std::clamp(
                std::lround((sourceAlpha * pixel[1] + glowWeight * colorG * 255.0) / outAlpha), 0L, 255L));
            pixel[2] = static_cast<uint8_t>(std::clamp(
                std::lround((sourceAlpha * pixel[2] + glowWeight * colorB2 * 255.0) / outAlpha), 0L, 255L));
            pixel[3] = static_cast<uint8_t>(std::clamp(std::lround(outAlpha * 255.0), 0L, 255L));
        }
    }
}

} // namespace FrameForge::Video
