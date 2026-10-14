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

/** @brief FrameForge native Twirl effect.
 *
 *  Adobe documents the Twirl effect (parity matrix section B5, archived
 *  distort-effects page): "The Twirl effect distorts an image by rotating
 *  a clip around its center. The image is distorted more sharply in its
 *  center than at the edges, causing a whirlpool result at extreme
 *  settings."
 *
 *  Parameters:
 *  - "angle": total rotation at the very center, in degrees (positive =
 *    clockwise).
 *  - "radius": twirl radius in pixels; pixels outside are untouched.
 *
 *  Implementation: inverse mapping — each destination pixel samples the
 *  source rotated around the frame center by angle * (1 - d / radius),
 *  where d is the pixel's distance from the center, so the rotation falls
 *  off to zero at the radius edge (the documented center-dominant
 *  distortion). Bilinear sampling; the frame is copied first so the
 *  remap never reads overwritten pixels. The rotation center is the
 *  frame center (documented v1 simplification).
 */
class Twirl final : public IVideoEffect
{
public:
    Twirl();

    std::string_view id() const override;
    std::string_view version() const override;
    std::vector<EffectParameterDescriptor> parameters() const override;
    void setParameter(const std::string &name, double value) override;
    double parameter(const std::string &name) const override;
    void prepare(int width, int height) override;
    void processFrame(uint8_t *rgba, size_t stride, int width, int height) override;

private:
    double m_angle{90.0};
    double m_radius{100.0};
    std::vector<uint8_t> m_copy; ///< Packed source copy (w*4 per row).
};

} // namespace FrameForge::Video
