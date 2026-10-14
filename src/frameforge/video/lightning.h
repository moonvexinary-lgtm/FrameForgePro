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

/** @brief FrameForge native Lightning effect.
 *
 *  Adobe documents the Lightning effect (parity matrix section B6,
 *  archived generate-effects page): "Creates animated lightning bolts,
 *  Jacob's Ladders, or other electric visuals between two specified
 *  points in a clip. The effect animates automatically across the clip's
 *  duration without keyframes."
 *
 *  Parameters:
 *  - "x1"/"y1"/"x2"/"y2": the two bolt endpoints as frame percentages.
 *  - "amplitude": maximum perpendicular displacement in pixels.
 *  - "segments": number of bolt segments (jag resolution).
 *  - "seed": evolution of the bolt shape — animate this over time to get
 *    the documented automatic animation (the engine passes media time at
 *    the wiring milestone; until then hosts animate the seed).
 *
 *  Implementation: the bolt is a polyline from start to end with each
 *  vertex displaced perpendicular to the bolt axis by deterministic
 *  hash noise, tapered to zero at the endpoints. Segments are drawn
 *  additively (white core) with a DDA line walker. All state is derived
 *  from the parameters (no allocation, no global RNG).
 */
class Lightning final : public IVideoEffect
{
public:
    Lightning();

    std::string_view id() const override;
    std::string_view version() const override;
    std::vector<EffectParameterDescriptor> parameters() const override;
    void setParameter(const std::string &name, double value) override;
    double parameter(const std::string &name) const override;
    void prepare(int width, int height) override;
    void processFrame(uint8_t *rgba, size_t stride, int width, int height) override;

private:
    double m_x1{25.0}, m_y1{25.0};
    double m_x2{75.0}, m_y2{75.0};
    double m_amplitude{30.0};
    double m_segments{12.0};
    double m_seed{0.0};
};

} // namespace FrameForge::Video
