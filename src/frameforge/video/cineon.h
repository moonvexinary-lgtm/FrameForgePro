/*
    This file is part of Kdenlive / the FrameForge Pro development branch
    SPDX-FileCopyrightText: 2026 FrameForge Pro contributors

    SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL
*/

#pragma once

#include "frameforge/interfaces/ivideoeffect.h"

#include <array>
#include <cstdint>
#include <string>
#include <vector>

namespace FrameForge::Video
{

/** @brief FrameForge native Cineon Converter effect.
 *
 *  Adobe documents the Cineon Converter effect (parity matrix section B16,
 *  archived utility-effects page): "The Cineon Converter effect provides a
 *  high degree of control over the color conversions of Cineon frames...
 *  The 10 bits of data available in each Cineon channel for each pixel make
 *  it easier to enhance an important range of tones while preserving
 *  overall tonal balance. By carefully specifying the range, you can
 *  create a version of the image that faithfully resembles the original."
 *
 *  The conversion uses the canonical Cineon 10-bit logarithmic encoding
 *  (Kodak Digital Film System reference values, all cross-checked):
 *  - reference white: code 685 (linear 1.0)
 *  - 18% gray: code ~470 (linear ~0.192 via 10^((c-685)/300))
 *  - reference black: code 95 with a linear toe from 0 to the log value
 *    10^((95-685)/300) ~= 0.0108 (the well-known Cineon toe constant)
 *
 *  Parameters:
 *  - "conversion": 1 = Cineon Log -> Linear (default), 0 = Linear -> Cineon Log.
 *  - "black_point": 10-bit code of reference black (default 95).
 *  - "white_point": 10-bit code of reference white (default 685).
 *  - "gamma": gamma applied to the linear result (Log->Linear) or to the
 *    input before encoding (Linear->Log); 1.0 = neutral.
 *
 *  Implementation: per-channel 256-entry LUT rebuilt on parameter change
 *  (8-bit frames; the 10-bit code domain is v8 * 1023 / 255). RGB only;
 *  alpha passes through.
 */
class CineonConverter final : public IVideoEffect
{
public:
    CineonConverter();

    std::string_view id() const override;
    std::string_view version() const override;
    std::vector<EffectParameterDescriptor> parameters() const override;
    void setParameter(const std::string &name, double value) override;
    double parameter(const std::string &name) const override;
    void prepare(int width, int height) override;
    void processFrame(uint8_t *rgba, size_t stride, int width, int height) override;

private:
    void rebuildTable();

    double m_conversion{1.0};   ///< 1 = Log->Linear, 0 = Linear->Log
    double m_blackPoint{95.0};  ///< 10-bit reference black
    double m_whitePoint{685.0}; ///< 10-bit reference white
    double m_gamma{1.0};

    std::array<uint8_t, 256> m_lut{};
};

} // namespace FrameForge::Video
