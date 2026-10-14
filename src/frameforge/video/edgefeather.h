/*
    This file is part of Kdenlive / the FrameForge Pro development branch
    SPDX-FileCopyrightText: 2026 FrameForge Pro contributors

    SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL
*/

#pragma once

#include "frameforge/interfaces/ivideoeffect.h"

#include <string>
#include <vector>

namespace FrameForge::Video
{

/** @brief FrameForge native Edge Feather effect.
 *
 *  Adobe documents the Edge Feather effect (parity matrix section B14,
 *  archived transform-effects page): "The effect adds a soft black border
 *  around your video, giving it a subtle vignette look. You can adjust the
 *  Amount value to control the width of the border."
 *
 *  Implementation: the alpha channel is feathered by blurring it with a
 *  separable box blur whose radius is derived from the "amount" parameter
 *  (the border width in pixels; radius = ceil(amount/2) so the total
 *  softening width matches the amount). When the clip is composited over a
 *  background, the softened alpha produces exactly the documented soft-edge
 *  look; over black it yields the described soft black border.
 *
 *  Parameters:
 *  - "amount": border/feather width in pixels (0 = bypass).
 */
class EdgeFeather final : public IVideoEffect
{
public:
    EdgeFeather();

    std::string_view id() const override;
    std::string_view version() const override;
    std::vector<EffectParameterDescriptor> parameters() const override;
    void setParameter(const std::string &name, double value) override;
    double parameter(const std::string &name) const override;
    void prepare(int width, int height) override;
    void processFrame(uint8_t *rgba, size_t stride, int width, int height) override;

private:
    double m_amount{0.0};
    std::vector<float> m_temp;  ///< width*height scratch for the blur.
    std::vector<float> m_line;  ///< prefix-sum line scratch.
};

} // namespace FrameForge::Video
