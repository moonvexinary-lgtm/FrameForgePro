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

/** @brief FrameForge native Extract effect.
 *
 *  Adobe documents the Extract effect (parity matrix section B1, archived
 *  adjust-effects page): "The Extract effect removes colors from your video,
 *  turning it into grayscale. It makes all pixels with luminance value less
 *  than the Black Input Level or greater than the White Input Level appear
 *  black, while everything in between shows up as gray or white."
 *
 *  Parameters:
 *  - "black_input": Black Input Level (0-255 luminance).
 *  - "white_input": White Input Level (0-255 luminance).
 *  - "softness": softens the black and white cutoffs into linear ramps
 *    (classic Extract "Softness" control; 0 = hard cutoffs).
 *  - "invert": swaps the result (0 -> white, 1 -> black).
 *
 *  Luminance is computed with Rec.709 coefficients (0.2126 R, 0.7152 G,
 *  0.0722 B); the mapping is precomputed into a 256-entry lookup table on
 *  parameter change, so processing is allocation-free. Alpha passes through.
 */
class Extract final : public IVideoEffect
{
public:
    Extract();

    std::string_view id() const override;
    std::string_view version() const override;
    std::vector<EffectParameterDescriptor> parameters() const override;
    void setParameter(const std::string &name, double value) override;
    double parameter(const std::string &name) const override;
    void prepare(int width, int height) override;
    void processFrame(uint8_t *rgba, size_t stride, int width, int height) override;

private:
    void rebuildTable();

    double m_blackInput{0.0};
    double m_whiteInput{255.0};
    double m_softness{0.0};
    bool m_invert{false};

    std::array<uint8_t, 256> m_lut{};
};

} // namespace FrameForge::Video
