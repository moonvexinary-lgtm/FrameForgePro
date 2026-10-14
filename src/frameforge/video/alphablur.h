/*
    This file is part of Kdenlive / the FrameForge Pro development branch
    SPDX-FileCopyrightText: 2026 FrameForge Pro contributors

    SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL
*/

#pragma once

#include <cstddef>
#include <cstdint>

namespace FrameForge::Video
{

/** @brief Separable box blur of the alpha channel of a packed RGBA8 frame.
 *
 *  Blurs only the alpha bytes (RGB untouched) using prefix sums: two linear
 *  passes (horizontal into `temp`, vertical back into the frame). Edges are
 *  clamped. Render-path safe: no allocation; the caller provides scratch
 *  buffers sized in prepare().
 *
 *  @param rgba    Frame buffer (packed 8-bit RGBA, straight alpha).
 *  @param stride  Bytes per row (>= width * 4).
 *  @param width   Frame width in pixels.
 *  @param height  Frame height in pixels.
 *  @param radius  Blur radius in pixels (window 2*radius+1); 0 = no-op.
 *  @param temp    Scratch, at least width*height floats (horizontal pass).
 *  @param line    Scratch, at least max(width, height)+1 floats (prefix sums).
 */
void boxBlurAlpha(uint8_t *rgba, size_t stride, int width, int height, int radius, float *temp, float *line);

} // namespace FrameForge::Video
