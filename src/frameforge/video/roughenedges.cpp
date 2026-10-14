/*
    This file is part of Kdenlive / the FrameForge Pro development branch
    SPDX-FileCopyrightText: 2026 FrameForge Pro contributors

    SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL
*/

#include "frameforge/video/roughenedges.h"

#include "frameforge/video/sample.h"

#include <algorithm>
#include <cmath>
#include <cstdint>

namespace
{
/// Deterministic 32-bit hash returning a value in the half-open range
/// from 0 to 1. Two channels come from two seed offsets; uint32 arithmetic
/// has well-defined wraparound (no UB, no platform drift, no global state).
double latticeValue(uint32_t ix, uint32_t iy, uint32_t seed)
{
    uint32_t h = ix * 374761393U + iy * 668265263U + seed * 69069U;
    h = (h ^ (h >> 13)) * 1274126177U;
    h = h ^ (h >> 16);
    return static_cast<double>(h & 0xFFFFFFU) / 16777215.0;
}

/// Smooth (smoothstep) fade for the value-noise interpolation.
double fade(double t)
{
    return t * t * (3.0 - 2.0 * t);
}

/** @brief Two-channel value noise at fractional lattice coordinates.
 *  Returns [0,1] in both channels; channel 2 uses a seed offset. */
void valueNoise(double x, double y, uint32_t seed, double out[2])
{
    const int ix = static_cast<int>(std::floor(x));
    const int iy = static_cast<int>(std::floor(y));
    const double tx = fade(x - static_cast<double>(ix));
    const double ty = fade(y - static_cast<double>(iy));
    const auto uix = static_cast<uint32_t>(ix);
    const auto uiy = static_cast<uint32_t>(iy);
    for (int c = 0; c < 2; ++c) {
        const uint32_t s = seed + static_cast<uint32_t>(c) * 7919U;
        const double v00 = latticeValue(uix, uiy, s);
        const double v10 = latticeValue(uix + 1, uiy, s);
        const double v01 = latticeValue(uix, uiy + 1, s);
        const double v11 = latticeValue(uix + 1, uiy + 1, s);
        const double top = v00 + (v10 - v00) * tx;
        const double bottom = v01 + (v11 - v01) * tx;
        out[c] = top + (bottom - top) * ty;
    }
}
} // namespace

namespace FrameForge::Video
{

RoughenEdges::RoughenEdges() = default;

std::string_view RoughenEdges::id() const
{
    return "frameforge.roughenedges";
}

std::string_view RoughenEdges::version() const
{
    return "1.0";
}

std::vector<EffectParameterDescriptor> RoughenEdges::parameters() const
{
    return {
        {"border", 4.0, 0.0, 32.0},      ///< Maximum edge displacement (pixels)
        {"scale", 50.0, 1.0, 1000.0},   ///< Noise lattice step (pixels)
        {"evolution", 0.0, 0.0, 1000.0}, ///< Animatable noise offset
    };
}

void RoughenEdges::setParameter(const std::string &name, double value)
{
    // Clamp to the declared ranges (host bugs must not produce nonsense).
    const auto descriptors = parameters();
    for (const auto &descriptor : descriptors) {
        if (descriptor.name == name) {
            value = std::clamp(value, descriptor.minimum, descriptor.maximum);
            break;
        }
    }
    if (name == "border") {
        m_border = value;
    } else if (name == "scale") {
        m_scale = value;
    } else if (name == "evolution") {
        m_evolution = value;
    } else {
        // Unknown parameter: ignored per the IVideoEffect contract.
    }
}

double RoughenEdges::parameter(const std::string &name) const
{
    if (name == "border") {
        return m_border;
    }
    if (name == "scale") {
        return m_scale;
    }
    if (name == "evolution") {
        return m_evolution;
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

void RoughenEdges::prepare(int width, int height)
{
    // Packed copy of the frame for the displaced sampling: sized here
    // (allocation is allowed in prepare, never in processFrame).
    m_copy.assign(static_cast<size_t>(std::max(1, width)) * 4 * static_cast<size_t>(std::max(1, height)), 0);
}

void RoughenEdges::processFrame(uint8_t *rgba, size_t stride, int width, int height)
{
    // No allocation, no locking: render path (see IVideoEffect contract).
    if (rgba == nullptr || width <= 0 || height <= 0 || stride < static_cast<size_t>(width) * 4) {
        return;
    }
    if (m_border <= 0.0) {
        return; // Identity: no displacement.
    }
    const size_t packedStride = static_cast<size_t>(width) * 4;
    if (m_copy.size() < packedStride * static_cast<size_t>(height)) {
        return; // Host changed geometry without prepare().
    }
    // Copy the source into the packed scratch (displaced reads must not
    // see already-roughened pixels).
    for (int y = 0; y < height; ++y) {
        std::copy_n(rgba + y * stride, packedStride, m_copy.data() + y * packedStride);
    }
    const double scale = std::max(1.0, m_scale);
    const auto seed = static_cast<uint32_t>(std::lround(m_evolution * 1024.0));
    for (int y = 0; y < height; ++y) {
        uint8_t *row = rgba + y * stride;
        for (int x = 0; x < width; ++x) {
            double noise[2];
            valueNoise(static_cast<double>(x) / scale, static_cast<double>(y) / scale, seed, noise);
            // Displace the sample by up to +/- border pixels.
            const double sx = static_cast<double>(x) + (noise[0] - 0.5) * 2.0 * m_border;
            const double sy = static_cast<double>(y) + (noise[1] - 0.5) * 2.0 * m_border;
            uint8_t sampled[4];
            sampleBilinear(m_copy.data(), packedStride, width, height, sx, sy, sampled);
            uint8_t *pixel = row + static_cast<size_t>(x) * 4;
            pixel[3] = sampled[3];
            // RGB passes through untouched.
        }
    }
}

} // namespace FrameForge::Video
