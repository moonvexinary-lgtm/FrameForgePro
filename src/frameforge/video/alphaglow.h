/*
    This file is part of Kdenlive / the FrameForge Pro development branch
    SPDX-FileCopyrightText: 2026 FrameForge Pro contributors

    SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL
*/

#pragma once

#include "frameforge/interfaces/ivideoeffect.h"

#include <cstdint>
#include <string>
#include <vector>

namespace FrameForge::Video
{

/** @brief FrameForge native Alpha Glow effect.
 *
 *  Adobe documents the Alpha Glow effect (parity matrix section B12,
 *  archived stylize-effects page): "The effect applies color along the
 *  edges of a masked alpha channel. You can make the color fade out or
 *  gradually shift to a second color as it extends from the edge."
 *
 *  Parameters:
 *  - "glow": how far the glow extends from the alpha edge, in pixels
 *    (used as the alpha blur radius; 0 = bypass).
 *  - "brightness": glow strength in percent (scales the glow's alpha).
 *  - "color_a": glow color near the alpha edge, packed 0xRRGGBB.
 *  - "color_b": glow color at the outer end of the glow, packed 0xRRGGBB.
 *    The color shifts from color_a to color_b as it extends from the edge;
 *    with both colors equal the glow simply fades out.
 *
 *  Implementation: the alpha channel is spread with the shared box blur
 *  (radius = "glow"); the glow mask is the blurred alpha minus the original
 *  alpha (the region outside the source alpha). The glow is composited
 *  under the original content using the standard over-composite for
 *  straight alpha, so interior pixels are untouched and transparent pixels
 *  receive the glow color with a falloff proportional to the spread.
 */
class AlphaGlow final : public IVideoEffect
{
public:
    AlphaGlow();

    std::string_view id() const override;
    std::string_view version() const override;
    std::vector<EffectParameterDescriptor> parameters() const override;
    void setParameter(const std::string &name, double value) override;
    double parameter(const std::string &name) const override;
    void prepare(int width, int height) override;
    void processFrame(uint8_t *rgba, size_t stride, int width, int height) override;

private:
    double m_glow{10.0};
    double m_brightness{100.0};
    double m_colorA{16777215.0}; ///< 0xFFFFFF white
    double m_colorB{16777215.0}; ///< 0xFFFFFF white

    std::vector<float> m_temp;       ///< width*height scratch for the blur.
    std::vector<float> m_line;       ///< prefix-sum line scratch.
    std::vector<uint8_t> m_origAlpha; ///< original alpha, width*height bytes.
};

} // namespace FrameForge::Video
