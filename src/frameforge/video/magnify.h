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

/** @brief FrameForge native Magnify effect.
 *
 *  Adobe documents the Magnify effect (parity matrix section B5, archived
 *  distort-effects page): "The effect lets you enlarge all or part of an
 *  image, like using a magnifying glass, or scale the whole image beyond
 *  100% without losing quality."
 *
 *  Parameters:
 *  - "center_x" / "center_y": magnifier center as a percentage of the
 *    frame dimensions.
 *  - "radius": circle radius in pixels; pixels outside are untouched.
 *  - "magnification": scale factor in percent (100 = identity).
 *
 *  Implementation: inverse mapping — destination pixels inside the circle
 *  sample the source at center + (dest - center) * 100/magnification, with
 *  bilinear interpolation over a packed frame copy. v1 uses a hard circle
 *  edge (no falloff), a documented simplification.
 */
class Magnify final : public IVideoEffect
{
public:
    Magnify();

    std::string_view id() const override;
    std::string_view version() const override;
    std::vector<EffectParameterDescriptor> parameters() const override;
    void setParameter(const std::string &name, double value) override;
    double parameter(const std::string &name) const override;
    void prepare(int width, int height) override;
    void processFrame(uint8_t *rgba, size_t stride, int width, int height) override;

private:
    double m_centerX{50.0};
    double m_centerY{50.0};
    double m_radius{100.0};
    double m_magnification{150.0};
    std::vector<uint8_t> m_copy; ///< Packed source copy (w*4 per row).
};

} // namespace FrameForge::Video
