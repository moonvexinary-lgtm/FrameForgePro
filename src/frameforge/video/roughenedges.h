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

/** @brief FrameForge native Roughen Edges effect.
 *
 *  Adobe documents the Roughen Edges effect (parity matrix section B12,
 *  archived stylize-effects page): "The Roughen Edges effect roughs up the
 *  edges of a clip's alpha channel by using calculations. It gives
 *  rasterized text or graphics a naturally rough look, like that of eroded
 *  metal or typewriter text."
 *
 *  Parameters:
 *  - "border": maximum edge displacement in pixels.
 *  - "scale": noise lattice step in pixels (smaller = finer detail).
 *  - "evolution": animatable offset through the noise field.
 *
 *  Implementation: each destination pixel samples the source alpha at a
 *  position displaced by two channels of deterministic hash-based value
 *  noise (uint32 arithmetic; no global RNG state, no platform drift), with
 *  bilinear interpolation over a packed frame copy. Only the alpha channel
 *  is modified — RGB passes through untouched.
 */
class RoughenEdges final : public IVideoEffect
{
public:
    RoughenEdges();

    std::string_view id() const override;
    std::string_view version() const override;
    std::vector<EffectParameterDescriptor> parameters() const override;
    void setParameter(const std::string &name, double value) override;
    double parameter(const std::string &name) const override;
    void prepare(int width, int height) override;
    void processFrame(uint8_t *rgba, size_t stride, int width, int height) override;

private:
    double m_border{4.0};
    double m_scale{50.0};
    double m_evolution{0.0};
    std::vector<uint8_t> m_copy; ///< Packed source copy (w*4 per row).
};

} // namespace FrameForge::Video
