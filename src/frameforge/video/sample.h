/*
    This file is part of Kdenlive / the FrameForge Pro development branch
    SPDX-FileCopyrightText: 2026 FrameForge Pro contributors

    SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL
*/

#pragma once

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <cstdint>

namespace FrameForge::Video
{

/** @brief Bilinear sample of a packed RGBA8 frame at fractional coordinates.
 *
 *  Coordinates are clamped to the frame. Alpha is interpolated like the
 *  color channels. Header-only inline helper for geometric remapping
 *  effects (Twirl, Spherize); render-path safe (no allocation).
 *
 *  @param rgba    Source frame (packed 8-bit RGBA).
 *  @param stride  Bytes per row of the source frame.
 *  @param width   Frame width in pixels.
 *  @param height  Frame height in pixels.
 *  @param fx      Fractional x coordinate (0..width-1).
 *  @param fy      Fractional y coordinate (0..height-1).
 *  @param out     Receives the interpolated RGBA byte values.
 */
inline void sampleBilinear(const uint8_t *rgba, size_t stride, int width, int height, double fx, double fy, uint8_t out[4])
{
    if (rgba == nullptr || width <= 0 || height <= 0) {
        out[0] = out[1] = out[2] = out[3] = 0;
        return;
    }
    fx = std::clamp(fx, 0.0, static_cast<double>(width - 1));
    fy = std::clamp(fy, 0.0, static_cast<double>(height - 1));
    const int x0 = static_cast<int>(std::floor(fx));
    const int y0 = static_cast<int>(std::floor(fy));
    const int x1 = std::min(x0 + 1, width - 1);
    const int y1 = std::min(y0 + 1, height - 1);
    const double tx = fx - static_cast<double>(x0);
    const double ty = fy - static_cast<double>(y0);
    const auto *p00 = rgba + static_cast<size_t>(y0) * stride + static_cast<size_t>(x0) * 4;
    const auto *p10 = rgba + static_cast<size_t>(y0) * stride + static_cast<size_t>(x1) * 4;
    const auto *p01 = rgba + static_cast<size_t>(y1) * stride + static_cast<size_t>(x0) * 4;
    const auto *p11 = rgba + static_cast<size_t>(y1) * stride + static_cast<size_t>(x1) * 4;
    for (int c = 0; c < 4; ++c) {
        const double top = p00[c] + (p10[c] - p00[c]) * tx;
        const double bottom = p01[c] + (p01[c] != p11[c] ? (p11[c] - p01[c]) * tx : 0.0);
        out[c] = static_cast<uint8_t>(std::clamp(std::lround(top + (bottom - top) * ty), 0L, 255L));
    }
}

} // namespace FrameForge::Video
