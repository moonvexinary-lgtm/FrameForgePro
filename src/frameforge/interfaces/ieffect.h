/*
    This file is part of Kdenlive / the FrameForge Pro development branch
    SPDX-FileCopyrightText: 2026 FrameForge Pro contributors

    SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL
*/

#pragma once

#include <string>
#include <string_view>
#include <vector>

namespace FrameForge
{

/** @brief Description of a single effect parameter.
 *
 *  Descriptors let a host build generic parameter UIs and let the future
 *  plugin registry validate effects without processing any audio.
 */
struct EffectParameterDescriptor
{
    std::string name; ///< Stable machine identifier, e.g. "frequency".
    double defaultValue{0.0};
    double minimum{0.0};
    double maximum{1.0};
};

/** @brief Base interface for FrameForge native effects.
 *
 *  This is the seam behind which the FrameForge engine's effect modules
 *  live. MLT remains the default effect backend until native effects are
 *  wired into the engine; implementing classes are proven by unit tests
 *  first (see tests/frameforge_dsptest.cpp).
 *
 *  Contract:
 *  - Audio is planar (non-interleaved) 32-bit float. All channels of a
 *    block have the same sample count.
 *  - prepare() must be called before the first process() call, and again
 *    whenever the sample rate or channel count changes. prepare() resets
 *    internal state.
 *  - setParameter() may be called between process() calls. It may allocate
 *    and reset filter state; hosts should call it at block boundaries.
 *  - process() runs on the audio path: it must not allocate, lock, log, or
 *    touch the filesystem.
 *  - process() is not thread-safe. An effect instance is owned by one
 *    processing chain and called from one thread at a time.
 */
class IEffect
{
public:
    virtual ~IEffect() = default;

    /// Stable effect identifier, e.g. "frameforge.dehummer".
    virtual std::string_view id() const = 0;
    /// Implementation version (interface/format compatibility).
    virtual std::string_view version() const = 0;
    /// Declared parameters with ranges and defaults.
    virtual std::vector<EffectParameterDescriptor> parameters() const = 0;
    /// Set a parameter by name (clamped to the declared range; unknown names are ignored).
    virtual void setParameter(const std::string &name, double value) = 0;
    /// Current parameter value (returns the default for unknown names).
    virtual double parameter(const std::string &name) const = 0;
    /// Configure for a stream and reset internal state.
    virtual void prepare(double sampleRate, size_t channelCount) = 0;
    /// Process one block of planar float samples in place.
    virtual void processPlanar(float *const *channels, size_t channelCount, size_t sampleCount) = 0;
};

} // namespace FrameForge
