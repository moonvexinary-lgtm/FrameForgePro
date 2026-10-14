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

/** @brief FrameForge native Levels effect.
 *
 *  Implements the classic Levels color-correction tool (Premiere "Levels",
 *  parity matrix section B1; Adobe documents it as giving "precise control
 *  over brightness and contrast" by combining color balance, gamma
 *  correction, brightness/contrast and invert functionality).
 *
 *  The transform is applied identically to the R, G and B channels; the
 *  alpha channel is passed through untouched.
 *
 *  Parameters (classic Levels semantics, component values in 0-255):
 *  - "input_black": input values at or below this map to output black.
 *  - "input_white": input values at or above this map to output white.
 *  - "gamma": midtone response; 1.0 is neutral, > 1 brightens midtones,
 *    < 1 darkens them.
 *  - "output_black": the black output level.
 *  - "output_white": the white output level.
 *
 *  Implementation: the per-component transform is precomputed into a
 *  256-entry lookup table whenever a parameter changes, so processing is a
 *  table lookup per component with no allocation on the render path.
 */
class Levels final : public IVideoEffect
{
public:
    Levels();

    std::string_view id() const override;
    std::string_view version() const override;
    std::vector<EffectParameterDescriptor> parameters() const override;
    void setParameter(const std::string &name, double value) override;
    double parameter(const std::string &name) const override;
    void prepare(int width, int height) override;
    void processFrame(uint8_t *rgba, size_t stride, int width, int height) override;

private:
    void rebuildTable();

    double m_inputBlack{0.0};
    double m_inputWhite{255.0};
    double m_gamma{1.0};
    double m_outputBlack{0.0};
    double m_outputWhite{255.0};

    std::array<uint8_t, 256> m_lut{};
};

} // namespace FrameForge::Video
