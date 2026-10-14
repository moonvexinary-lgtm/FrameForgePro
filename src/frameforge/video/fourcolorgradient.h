/*
    This file is part of Kdenlive / the FrameForge Pro development branch
    SPDX-FileCopyrightText: 2026 FrameForge Pro contributors

    SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL
*/

#pragma once

#include "frameforge/interfaces/ivideoeffect.h"

#include <cstdint>
#include <string>

namespace FrameForge::Video
{

/** @brief FrameForge native 4-Color Gradient effect.
 *
 *  Adobe documents the 4-Color Gradient effect (parity matrix section B6,
 *  archived generate-effects page): "Creates a gradient with four colors.
 *  Use the Positions and Colors controls to animate the positions and
 *  colors of the four effect points, blending them into solid-color
 *  circles, each with an effect point as its center."
 *
 *  Parameters (positions in percent of the frame, colors packed 0xRRGGBB):
 *  - "x1"/"y1" .. "x4"/"y4": the four effect-point positions
 *    (defaults: top-left, top-right, bottom-left, bottom-right).
 *  - "color1".."color4": the four colors (defaults: red, green,
 *    yellow, blue).
 *
 *  Implementation: each pixel is the normalized inverse-square blend of
 *  the four colors, so each effect point region dominates with its own
 *  solid color (the documented "solid-color circles") and colors merge
 *  smoothly in between. The gradient fills the frame (alpha set to 255);
 *  as a generator-style effect it replaces the RGB content.
 */
class FourColorGradient final : public IVideoEffect
{
public:
    FourColorGradient();

    std::string_view id() const override;
    std::string_view version() const override;
    std::vector<EffectParameterDescriptor> parameters() const override;
    void setParameter(const std::string &name, double value) override;
    double parameter(const std::string &name) const override;
    void prepare(int width, int height) override;
    void processFrame(uint8_t *rgba, size_t stride, int width, int height) override;

private:
    double m_x1{0.0}, m_y1{0.0};
    double m_x2{100.0}, m_y2{0.0};
    double m_x3{0.0}, m_y3{100.0};
    double m_x4{100.0}, m_y4{100.0};
    double m_color1{0xFF0000}; ///< red
    double m_color2{0x00FF00}; ///< green
    double m_color3{0xFFFF00}; ///< yellow
    double m_color4{0x0000FF}; ///< blue
};

} // namespace FrameForge::Video
