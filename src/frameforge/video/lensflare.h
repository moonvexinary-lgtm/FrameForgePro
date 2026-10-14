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

/** @brief FrameForge native Lens Flare effect.
 *
 *  Adobe documents the Lens Flare effect (parity matrix section B6,
 *  archived generate-effects page): "Simulates what happens when bright
 *  light hits a camera lens."
 *
 *  Parameters:
 *  - "center_x" / "center_y": flare center as a percentage of the frame.
 *  - "brightness": flare strength in percent.
 *  - "size": flare radius in pixels (core glow, halo ring and ghost
 *    scale with it).
 *
 *  Implementation: a synthetic flare is composited additively over the
 *  frame: a smooth core glow at the flare center, a thin halo ring at
 *  60% of the size, and a secondary ghost blob mirrored through the
 *  frame center (classic lens reflections). V1 uses this three-element
 *  model — a documented simplification of real lens geometry.
 */
class LensFlare final : public IVideoEffect
{
public:
    LensFlare();

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
    double m_brightness{100.0};
    double m_size{100.0};
};

} // namespace FrameForge::Video
