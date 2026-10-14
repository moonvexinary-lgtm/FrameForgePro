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

/** @brief FrameForge native Replicate effect.
 *
 *  Adobe documents the Replicate effect (parity matrix section B12,
 *  archived stylize-effects page): "The effect divides the screen into
 *  tiles and displays the whole image in each tile. Adjust the number of
 *  tiles in each column and row by moving the slider."
 *
 *  Parameters:
 *  - "horizontal": number of tiles per row (1-16).
 *  - "vertical": number of tiles per column (1-16).
 *
 *  Implementation: the frame is divided into a grid of
 *  ceil(width/horizontal) x ceil(height/vertical) cells; each cell shows
 *  a scaled-down copy of the whole frame (bilinear, over a packed frame
 *  copy so tiles never read overwritten pixels). 1x1 tiles = identity.
 */
class Replicate final : public IVideoEffect
{
public:
    Replicate();

    std::string_view id() const override;
    std::string_view version() const override;
    std::vector<EffectParameterDescriptor> parameters() const override;
    void setParameter(const std::string &name, double value) override;
    double parameter(const std::string &name) const override;
    void prepare(int width, int height) override;
    void processFrame(uint8_t *rgba, size_t stride, int width, int height) override;

private:
    double m_horizontal{2.0};
    double m_vertical{2.0};
    std::vector<uint8_t> m_copy; ///< Packed source copy (w*4 per row).
};

} // namespace FrameForge::Video
