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

/** @brief FrameForge native Spherize effect.
 *
 *  Adobe documents the Spherize effect (parity matrix section B5, archived
 *  distort-effects page): "The Spherize effect distorts a layer by
 *  wrapping a region of the image onto a sphere."
 *
 *  Parameters:
 *  - "amount": bulge strength in percent (0 = identity).
 *  - "radius_percent": sphere radius as a percentage of half the
 *    smaller frame dimension.
 *
 *  Implementation: inverse mapping — each destination pixel samples the
 *  source at an offset pulled toward the center by
 *  offset / (1 + strength * (1 - d^2)), where d is the normalized
 *  distance from the center (0..1 inside the radius). The mapping is
 *  seamless at the radius boundary (factor 1), magnifies the center for
 *  strength > 0 (the "wrapped onto a sphere" look), and leaves pixels
 *  outside the radius untouched. Bilinear sampling over a packed frame
 *  copy. (Adobe's exact internal curve is not documented; this bulge
 *  model is the documented v1 approximation — see the parity matrix.)
 */
class Spherize final : public IVideoEffect
{
public:
    Spherize();

    std::string_view id() const override;
    std::string_view version() const override;
    std::vector<EffectParameterDescriptor> parameters() const override;
    void setParameter(const std::string &name, double value) override;
    double parameter(const std::string &name) const override;
    void prepare(int width, int height) override;
    void processFrame(uint8_t *rgba, size_t stride, int width, int height) override;

private:
    double m_amount{50.0};
    double m_radiusPercent{100.0};
    std::vector<uint8_t> m_copy; ///< Packed source copy (w*4 per row).
};

} // namespace FrameForge::Video
