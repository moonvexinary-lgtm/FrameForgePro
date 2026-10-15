/*
    This file is part of Kdenlive / the FrameForge Pro development branch
    SPDX-FileCopyrightText: 2026 FrameForge Pro contributors

    SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL
*/

#include "frameforge/video/alphablur.h"

#include <algorithm>
#include <cmath>

namespace FrameForge::Video
{

void boxBlurAlpha(uint8_t *rgba, size_t stride, int width, int height, int radius, float *temp, float *line)
{
    if (rgba == nullptr || temp == nullptr || line == nullptr || width <= 0 || height <= 0 || radius <= 0) {
        return;
    }
    if (stride < static_cast<size_t>(width) * 4) {
        return;
    }

    // Horizontal pass: alpha -> temp, using per-row prefix sums with clamped
    // windows at the borders.
    for (int y = 0; y < height; ++y) {
        const uint8_t *row = rgba + y * stride;
        float *prefix = line;
        prefix[0] = 0.0f;
        for (int x = 0; x < width; ++x) {
            prefix[x + 1] = prefix[x] + static_cast<float>(row[static_cast<size_t>(x) * 4 + 3]);
        }
        float *out = temp + static_cast<size_t>(y) * width;
        for (int x = 0; x < width; ++x) {
            const int left = std::max(0, x - radius);
            const int right = std::min(width - 1, x + radius);
            const int count = right - left + 1;
            out[x] = (prefix[right + 1] - prefix[left]) / static_cast<float>(count);
        }
    }

    // Vertical pass: temp -> alpha, using per-column prefix sums.
    for (int x = 0; x < width; ++x) {
        float *prefix = line;
        prefix[0] = 0.0f;
        for (int y = 0; y < height; ++y) {
            prefix[y + 1] = prefix[y] + temp[static_cast<size_t>(y) * width + x];
        }
        for (int y = 0; y < height; ++y) {
            const int top = std::max(0, y - radius);
            const int bottom = std::min(height - 1, y + radius);
            const int count = bottom - top + 1;
            uint8_t *pixel = rgba + y * stride + static_cast<size_t>(x) * 4;
            pixel[3] = static_cast<uint8_t>(std::clamp(std::lroundf((prefix[bottom + 1] - prefix[top]) / static_cast<float>(count)), 0L, 255L));
        }
    }
}

} // namespace FrameForge::Video
