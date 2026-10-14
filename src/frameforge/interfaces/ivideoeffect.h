/*
    This file is part of Kdenlive / the FrameForge Pro development branch
    SPDX-FileCopyrightText: 2026 FrameForge Pro contributors

    SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL
*/

#pragma once

#include "frameforge/interfaces/ieffect.h"

#include <cstdint>
#include <string>
#include <string_view>
#include <vector>

namespace FrameForge
{

/** @brief Base interface for FrameForge native video effects.
 *
 *  This is the seam behind which the FrameForge engine's video effect
 *  modules live. MLT remains the default effect backend until native
 *  effects are wired into the engine; implementing classes are proven by
 *  unit tests first (see tests/frameforge_videodsptest.cpp).
 *
 *  Contract:
 *  - v1 frame format: packed 8-bit RGBA, straight (non-premultiplied)
 *    alpha, top-down rows, one row per `stride` bytes (stride >= width*4).
 *    The engine will extend this to planar and floating-point formats at
 *    the native-rendering milestone (M4).
 *  - prepare() must be called before the first processFrame() call, and
 *    again whenever the frame geometry changes. prepare() resets internal
 *    state. Effects must not retain the frame pointer.
 *  - setParameter() may be called between processFrame() calls. It may
 *    allocate and rebuild internal tables; hosts should call it at block
 *    boundaries.
 *  - processFrame() runs on the render path: it must not allocate, lock,
 *    log, or touch the filesystem.
 *  - processFrame() is not thread-safe. One effect instance is owned by
 *    one processing chain and called from one thread at a time.
 */
class IVideoEffect
{
public:
    virtual ~IVideoEffect() = default;

    /// Stable effect identifier, e.g. "frameforge.levels".
    virtual std::string_view id() const = 0;
    /// Implementation version (interface/format compatibility).
    virtual std::string_view version() const = 0;
    /// Declared parameters with ranges and defaults.
    virtual std::vector<EffectParameterDescriptor> parameters() const = 0;
    /// Set a parameter by name (clamped to the declared range; unknown names are ignored).
    virtual void setParameter(const std::string &name, double value) = 0;
    /// Current parameter value (returns the default for unknown names).
    virtual double parameter(const std::string &name) const = 0;
    /// Configure for a frame geometry and reset internal state.
    virtual void prepare(int width, int height) = 0;
    /// Process one frame of packed 8-bit RGBA in place.
    virtual void processFrame(uint8_t *rgba, size_t stride, int width, int height) = 0;
};

} // namespace FrameForge
